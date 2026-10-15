// The bounded task helper. It has no provider credentials or control-plane API.
package main

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/bmatcuk/doublestar/v4"
)

const maxBytes = 48 * 1024 * 1024
const maxFiles = 8192
const maxLog = 2 * 1024 * 1024

type Task struct {
	ID          string            `json:"id"`
	Name        string            `json:"name"`
	Description string            `json:"description"`
	Command     string            `json:"command"`
	Cwd         string            `json:"cwd"`
	Timeout     int               `json:"timeout_seconds"`
	Env         map[string]string `json:"env"`
	Products    []string          `json:"products"`
	Cache       []string          `json:"cache"`
	Reports     []string          `json:"reports"`
}
type Input struct {
	Task  Task              `json:"task"`
	Files map[string]string `json:"files"`
	Cache map[string]string `json:"cache"`
}
type Result struct {
	CacheSkipped bool              `json:"cache_skipped,omitempty"`
	Status       string            `json:"status"`
	ExitCode     *int              `json:"exit_code"`
	Started      int64             `json:"started_at"`
	Finished     int64             `json:"finished_at"`
	Log          string            `json:"log"`
	Complete     bool              `json:"complete"`
	Error        string            `json:"error,omitempty"`
	Files        map[string]string `json:"files"`
	Deleted      []string          `json:"deleted"`
	Products     []string          `json:"products"`
	Reports      map[string]string `json:"reports"`
	Cache        map[string]string `json:"cache"`
}

func safe(path string) bool {
	if path == "" || len(path) > 512 || strings.ContainsAny(path, "\\\x00:\r\n") || strings.HasPrefix(path, "/") {
		return false
	}
	for _, part := range strings.Split(path, "/") {
		if part == "" || part == "." || part == ".." {
			return false
		}
	}
	return true
}
func protected(path string) bool {
	for _, part := range strings.Split(path, "/") {
		if part == ".git" || part == ".ssh" || part == ".aws" || part == ".env" || strings.HasSuffix(part, ".env") || strings.HasSuffix(part, ".pem") || strings.HasSuffix(part, ".key") || part == "id_rsa" || part == "id_ed25519" {
			return true
		}
	}
	return strings.HasPrefix(path, ".jazyk/")
}
func inputAllowed(path string) bool {
	return !protected(path) || path == ".jazyk/jazyk.toml" || path == ".jazyk/.gitignore"
}
func matches(patterns []string, name string) bool {
	for _, pattern := range patterns {
		if yes, err := doublestar.Match(pattern, name); err == nil && yes {
			return true
		}
	}
	return false
}
func writeFiles(root string, files map[string]string, cache bool, input Input) error {
	total := 0
	if len(files) > maxFiles {
		return errors.New("too many input files")
	}
	for name, encoded := range files {
		if !safe(name) || !inputAllowed(name) {
			return errors.New("invalid or protected input path")
		}
		_, canonical := input.Files[name]
		if cache && (canonical || !matches(input.Task.Cache, name) || matches(input.Task.Reports, name) || matches(input.Task.Products, name)) {
			return errors.New("cache overlaps input or retained outputs")
		}
		data, err := base64.StdEncoding.DecodeString(encoded)
		if err != nil {
			return errors.New("invalid input encoding")
		}
		total += len(data)
		if total > maxBytes {
			return errors.New("input byte limit exceeded")
		}
		path := filepath.Join(root, filepath.FromSlash(name))
		if err = os.MkdirAll(filepath.Dir(path), 0755); err != nil {
			return err
		}
		if err = os.WriteFile(path, data, 0644); err != nil {
			return err
		}
	}
	return nil
}

// Redact exact credential values before streaming, including split writes.
type secretWriter struct {
	target  io.Writer
	secrets []string
	pending []byte
	keep    int
}

func (w *secretWriter) Write(p []byte) (int, error) {
	w.pending = append(w.pending, p...)
	w.flush(false)
	return len(p), nil
}
func (w *secretWriter) flush(final bool) {
	safe := len(w.pending) - w.keep + 1
	if final {
		safe = len(w.pending)
	}
	var out bytes.Buffer
	i := 0
	for i < safe && i < len(w.pending) {
		matched := false
		for _, s := range w.secrets {
			if bytes.HasPrefix(w.pending[i:], []byte(s)) {
				out.WriteString("[REDACTED]")
				i += len(s)
				matched = true
				break
			}
		}
		if !matched {
			out.WriteByte(w.pending[i])
			i++
		}
	}
	if i > 0 {
		_, _ = w.target.Write(out.Bytes())
		w.pending = append([]byte(nil), w.pending[i:]...)
	}
}
func runtimeSecrets() (map[string]bool, []string) {
	var keys []string
	_ = json.Unmarshal([]byte(os.Getenv("JAZYK_SECRET_KEYS")), &keys)
	names := map[string]bool{}
	values := []string{}
	if len(keys) > 20 {
		return names, values
	}
	for _, key := range keys {
		if key != "" && !strings.HasPrefix(key, "JAZYK_") {
			if value := os.Getenv(key); value != "" && len(value) <= 4096 {
				names[key] = true
				values = append(values, value)
			}
		}
	}
	sort.Slice(values, func(i, j int) bool { return len(values[i]) > len(values[j]) })
	return names, values
}

type logWriter struct {
	mu       sync.Mutex
	buf      bytes.Buffer
	overflow chan struct{}
	once     sync.Once
	stream   io.Writer
}

func (w *logWriter) Write(p []byte) (int, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	n := len(p)
	if len(p) > maxLog-w.buf.Len() {
		p = p[:maxLog-w.buf.Len()]
		w.once.Do(func() { close(w.overflow) })
	}
	w.buf.Write(p)
	if w.stream != nil {
		_, _ = w.stream.Write(p)
	}
	return n, nil
}
func execute(input Input, root string, stream io.Writer, cancel <-chan os.Signal) Result {
	r := Result{Status: "failed", Started: time.Now().Unix(), Complete: true, Files: map[string]string{}, Reports: map[string]string{}, Cache: map[string]string{}, Deleted: []string{}, Products: []string{}}
	if input.Task.Timeout < 1 || input.Task.Timeout > 7200 || strings.TrimSpace(input.Task.Command) == "" {
		r.Error = "invalid task command or timeout"
		return r
	}
	if input.Task.Cwd == "" {
		input.Task.Cwd = "."
	}
	if input.Task.Cwd != "." && !safe(input.Task.Cwd) {
		r.Error = "invalid working directory"
		return r
	}
	if err := writeFiles(root, input.Files, false, input); err != nil {
		r.Error = err.Error()
		return r
	}
	if err := writeFiles(root, input.Cache, true, input); err != nil {
		r.Error = err.Error()
		return r
	}
	// Previous generated reports are not evidence from this attempt.
	for name := range input.Files {
		if matches(input.Task.Reports, name) {
			_ = os.Remove(filepath.Join(root, filepath.FromSlash(name)))
		}
	}
	secretNames, secretValues := runtimeSecrets()
	masked := &secretWriter{target: stream, secrets: secretValues, keep: 1}
	for _, value := range secretValues {
		if len(value) > masked.keep {
			masked.keep = len(value)
		}
	}
	if stream == nil {
		masked.target = io.Discard
	}
	log := &logWriter{overflow: make(chan struct{}), stream: masked}
	cmd := exec.Command("/bin/sh", "-c", input.Task.Command)
	cmd.Dir = filepath.Join(root, input.Task.Cwd)
	cmd.Env = []string{}
	for _, v := range os.Environ() {
		key, _, _ := strings.Cut(v, "=")
		if !strings.HasPrefix(key, "JAZYK_") && (!strings.HasPrefix(key, "AWS_") || secretNames[key]) {
			cmd.Env = append(cmd.Env, v)
		}
	}
	for key, value := range input.Task.Env {
		if strings.ContainsAny(key, "=\x00") || strings.HasPrefix(key, "AWS_") || strings.HasPrefix(key, "JAZYK_") {
			r.Error = "invalid environment key"
			return r
		}
		cmd.Env = append(cmd.Env, key+"="+value)
	}
	cmd.WaitDelay = 2 * time.Second
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	cmd.Stdout = log
	cmd.Stderr = log
	if err := cmd.Start(); err != nil {
		r.Error = "command could not start: " + err.Error()
		return r
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	timer := time.NewTimer(time.Duration(input.Task.Timeout) * time.Second)
	defer timer.Stop()
	finished := false
	select {
	case err := <-done:
		finished = true
		if err == nil || (errors.Is(err, exec.ErrWaitDelay) && cmd.ProcessState.ExitCode() == 0) {
			r.Status = "succeeded"
		} else {
			r.Status = "failed"
		}
	case <-timer.C:
		r.Status = "timed_out"
	case <-cancel:
		r.Status = "cancelled"
	case <-log.overflow:
		r.Status = "failed"
		r.Error = "log limit exceeded"
	}
	if !finished {
		_ = syscall.Kill(-cmd.Process.Pid, syscall.SIGTERM)
		select {
		case <-done:
		case <-time.After(2 * time.Second):
			_ = syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
			<-done
		}
	}
	_ = syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
	// PID 1 is the container supervisor. Kill detached writers before collection.
	if os.Getpid() == 1 {
		_ = syscall.Kill(-1, syscall.SIGKILL)
	}
	code := cmd.ProcessState.ExitCode()
	r.ExitCode = &code
	log.mu.Lock()
	r.Log = log.buf.String()
	for _, value := range secretValues {
		r.Log = strings.ReplaceAll(r.Log, value, "[REDACTED]")
	}
	masked.flush(true)
	log.mu.Unlock()
	if err := collect(input, root, &r); err != nil {
		r.Complete = false
		r.Error = err.Error()
	}
	r.Finished = time.Now().Unix()
	return r
}
func readRetained(path string, limit int64) ([]byte, error) {
	fd, err := syscall.Open(path, syscall.O_RDONLY|syscall.O_NOFOLLOW|syscall.O_NONBLOCK, 0)
	if err != nil {
		return nil, err
	}
	f := os.NewFile(uintptr(fd), path)
	defer f.Close()
	info, err := f.Stat()
	if err != nil || !info.Mode().IsRegular() {
		return nil, errors.New("output changed during capture")
	}
	if info.Size() > limit {
		return nil, errors.New("retained byte limit reached")
	}
	data, err := io.ReadAll(io.LimitReader(f, limit+1))
	if int64(len(data)) > limit {
		return nil, errors.New("retained byte limit reached")
	}
	return data, err
}
func collect(input Input, root string, r *Result) error {
	total, count := 0, 0
	deadline := time.Now().Add(30 * time.Second)
	seen := map[string]bool{}
	cachePaths := []string{}
	err := filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if path == root {
			return nil
		}
		name, e := filepath.Rel(root, path)
		if e != nil {
			return e
		}
		name = filepath.ToSlash(name)
		if !safe(name) || protected(name) {
			if d.IsDir() {
				return filepath.SkipDir
			}
			return nil
		}
		if time.Now().After(deadline) {
			return errors.New("collection deadline reached")
		}
		count++
		if count > maxFiles*8 {
			return errors.New("file scan limit reached")
		}
		if d.IsDir() || d.Type()&os.ModeSymlink != 0 {
			return nil
		}
		info, e := d.Info()
		if e != nil {
			return e
		}
		if !info.Mode().IsRegular() {
			return nil
		}
		_, exists := input.Files[name]
		seen[name] = true
		product, report, cache := matches(input.Task.Products, name), matches(input.Task.Reports, name), matches(input.Task.Cache, name)
		if (product && cache) || (report && cache) || (product && report) {
			return errors.New("file matches conflicting output classes")
		}
		// Canonical inputs remain inputs even if an overbroad cache pattern matches.
		if cache && !exists {
			if r.Status == "succeeded" {
				cachePaths = append(cachePaths, name)
			}
			return nil
		}
		if !exists && !product && !report {
			return nil
		}
		data, e := readRetained(path, maxBytes)
		if e != nil {
			return e
		}
		encoded := base64.StdEncoding.EncodeToString(data)
		if !report && !product && encoded == input.Files[name] {
			return nil
		}
		if total+len(data) > maxBytes || len(r.Files)+len(r.Reports) >= maxFiles {
			return errors.New("retained output limit reached")
		}
		total += len(data)
		if report {
			r.Reports[name] = encoded
		} else {
			r.Files[name] = encoded
			if product {
				r.Products = append(r.Products, name)
			}
		}
		return nil
	})
	// Cache cannot consume the product budget or turn successful output into a partial run.
	if err == nil {
		for _, name := range cachePaths {
			if time.Now().After(deadline) || len(r.Files)+len(r.Reports)+len(r.Cache) >= maxFiles {
				r.CacheSkipped = true
				break
			}
			data, e := readRetained(filepath.Join(root, filepath.FromSlash(name)), int64(maxBytes-total))
			if e != nil {
				r.CacheSkipped = true
				break
			}
			total += len(data)
			r.Cache[name] = base64.StdEncoding.EncodeToString(data)
		}
	}
	if r.CacheSkipped {
		r.Cache = map[string]string{}
	}
	for name := range input.Files {
		if !seen[name] && !protected(name) && !matches(input.Task.Reports, name) {
			r.Deleted = append(r.Deleted, name)
		}
	}
	sort.Strings(r.Deleted)
	sort.Strings(r.Products)
	return err
}
func fetch(location string) ([]byte, error) {
	if !strings.HasPrefix(location, "https://") {
		return os.ReadFile(location)
	}
	client := &http.Client{Timeout: 60 * time.Second}
	resp, err := client.Get(location)
	if err != nil {
		return nil, errors.New("input download failed")
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return nil, fmt.Errorf("input download status %d", resp.StatusCode)
	}
	data, err := io.ReadAll(io.LimitReader(resp.Body, 128*1024*1024+1))
	if len(data) > 128*1024*1024 {
		return nil, errors.New("input transfer limit reached")
	}
	return data, err
}
func store(location string, data []byte) error {
	if !strings.HasPrefix(location, "https://") {
		return os.WriteFile(location, data, 0600)
	}
	req, err := http.NewRequest(http.MethodPut, location, bytes.NewReader(data))
	if err != nil {
		return errors.New("invalid output transfer")
	}
	req.Header.Set("Content-Type", "application/json")
	client := &http.Client{Timeout: 60 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		return errors.New("output upload failed")
	}
	defer resp.Body.Close()
	if resp.StatusCode/100 != 2 {
		return fmt.Errorf("output upload status %d", resp.StatusCode)
	}
	return nil
}
func main() {
	if len(os.Args) == 3 && os.Args[1] == "install" {
		data, err := os.ReadFile("/runner")
		if err == nil {
			err = os.WriteFile(filepath.Join(os.Args[2], "runner"), data, 0755)
		}
		if err == nil {
			var roots []byte
			roots, err = os.ReadFile("/ca.crt")
			if err == nil {
				err = os.WriteFile(filepath.Join(os.Args[2], "ca.crt"), roots, 0644)
			}
		}
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		return
	}
	inputURL, outputURL := os.Getenv("JAZYK_INPUT"), os.Getenv("JAZYK_OUTPUT")
	root := os.Getenv("JAZYK_WORKSPACE")
	if root == "" {
		root = "/project"
	}
	result := Result{Status: "failed", Complete: false, Started: time.Now().Unix()}
	data, err := fetch(inputURL)
	var input Input
	if err == nil {
		err = json.Unmarshal(data, &input)
	}
	if err == nil {
		err = os.MkdirAll(root, 0755)
	}
	signals := make(chan os.Signal, 1)
	signal.Notify(signals, syscall.SIGTERM, syscall.SIGINT)
	if err == nil {
		result = execute(input, root, os.Stdout, signals)
	} else {
		result.Error = err.Error()
	}
	result.Finished = time.Now().Unix()
	data, _ = json.Marshal(result)
	if err = store(outputURL, data); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
	if result.Status != "succeeded" {
		os.Exit(1)
	}
}
