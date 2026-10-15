package main

import (
	"encoding/base64"
	"os"
	"syscall"
	"testing"
	"time"
)

func task(command string) Input {
	return Input{Task: Task{ID: "check", Command: command, Timeout: 5, Cwd: ".", Products: []string{"dist/**"}, Cache: []string{".cache/**"}, Reports: []string{"reports/*.xml"}}, Files: map[string]string{}, Cache: map[string]string{}}
}
func encoded(s string) string { return base64.StdEncoding.EncodeToString([]byte(s)) }
func TestProductsAndDisposableCache(t *testing.T) {
	input := task("mkdir -p dist .cache; echo product > dist/book.pdf; echo dependency > .cache/lib; echo transient > scratch")
	result := execute(input, t.TempDir(), nil, make(chan os.Signal))
	if result.Status != "succeeded" || len(result.Products) != 1 || len(result.Cache) != 1 || len(result.Files) != 1 {
		t.Fatalf("wrong retention: %+v", result)
	}
	input.Task.Command += "; exit 1"
	result = execute(input, t.TempDir(), nil, make(chan os.Signal))
	if result.Status != "failed" || len(result.Cache) != 0 || len(result.Products) != 1 {
		t.Fatalf("failed task retention: %+v", result)
	}
}
func TestOldReportsCannotBecomeFreshEvidence(t *testing.T) {
	input := task("exit 1")
	input.Files["reports/old.xml"] = encoded("<testsuite><testcase name='old'/></testsuite>")
	result := execute(input, t.TempDir(), nil, make(chan os.Signal))
	if len(result.Reports) != 0 {
		t.Fatal("old reports survived")
	}
}
func TestCancelAndTimeoutStopCommands(t *testing.T) {
	for _, cancelled := range []bool{true, false} {
		input := task("sleep 30")
		input.Task.Timeout = 1
		signals := make(chan os.Signal, 1)
		if cancelled {
			go func() { time.Sleep(100 * time.Millisecond); signals <- syscall.SIGTERM }()
		}
		start := time.Now()
		result := execute(input, t.TempDir(), nil, signals)
		expected := "timed_out"
		if cancelled {
			expected = "cancelled"
		}
		if result.Status != expected || time.Since(start) > 4*time.Second {
			t.Fatalf("command did not stop: %+v", result)
		}
	}
}
func TestSymlinkAndEmptyInputCacheCollision(t *testing.T) {
	input := task("mkdir dist; ln -s /etc/passwd dist/secret")
	result := execute(input, t.TempDir(), nil, make(chan os.Signal))
	if len(result.Files) != 0 {
		t.Fatal("symlink contents retained")
	}
	input = task("true")
	input.Files[".cache/empty"] = ""
	input.Cache[".cache/empty"] = encoded("replacement")
	result = execute(input, t.TempDir(), nil, make(chan os.Signal))
	if result.Error == "" {
		t.Fatal("cache overwrote empty canonical file")
	}
}
func TestBackgroundChildDoesNotHoldResultOpen(t *testing.T) {
	start := time.Now()
	result := execute(task("sleep 30 & echo finished"), t.TempDir(), nil, make(chan os.Signal))
	if result.Status != "succeeded" || time.Since(start) > 4*time.Second {
		t.Fatalf("background child held result: %+v", result)
	}
}
