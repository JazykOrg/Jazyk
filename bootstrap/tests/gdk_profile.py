"""Sample the opt-in GDK profile process. Uses ps on macOS; no model service.

Build: cargo build --release --no-default-features --example gdk_profile
Run: python3 tests/gdk_profile.py --binary target/release/examples/gdk_profile
"""
import argparse
from datetime import datetime, timezone
import json
import platform
from pathlib import Path
import queue
import subprocess
import threading
import time


def sample(pid):
    rss = subprocess.run(['/bin/ps', '-o', 'rss=', '-p', str(pid)],
                         capture_output=True, text=True).stdout.strip()
    threads = subprocess.run(['/bin/ps', '-M', '-p', str(pid)],
                             capture_output=True, text=True).stdout.splitlines()
    return {'rssKiB': int(rss) if rss.isdigit() else None,
            'threads': max(0, len(threads) - 1)}


def run(binary, active, cancel_every, idle=100):
    process = subprocess.Popen([binary, '--active', str(active), '--idle', str(idle),
                                '--cancel-every', str(cancel_every)],
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    lines = queue.Queue()

    def read():
        for line in process.stdout:
            lines.put(json.loads(line))

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    phase = 'prepare'
    baseline, live = [], []
    ready, result = None, None
    while process.poll() is None:
        while not lines.empty():
            event = lines.get_nowait()
            if event['event'] == 'ready':
                ready, phase = event, 'idle'
            elif event['event'] == 'running':
                phase = 'active'
            elif event['event'] == 'result':
                result = event
        measurement = sample(process.pid)
        if measurement['rssKiB'] is not None:
            if phase == 'idle':
                baseline.append(measurement)
            elif phase == 'active':
                live.append(measurement)
        time.sleep(0.01)
    reader.join()
    while not lines.empty():
        event = lines.get_nowait()
        if event['event'] == 'result':
            result = event
    error = process.stderr.read()
    if process.returncode or result is None or ready is None:
        raise RuntimeError(f'profile failed ({process.returncode}): {error}')
    result.update({'idleCapsuleBytes': ready['idleCapsuleBytes'],
                   'idleRssKiB': max((item['rssKiB'] for item in baseline), default=None),
                   'activePeakRssKiB': max((item['rssKiB'] for item in live), default=None),
                   'idleThreads': max((item['threads'] for item in baseline), default=None),
                   'activePeakThreads': max((item['threads'] for item in live), default=None),
                   'rssSampleCount': len(baseline) + len(live)})
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--output')
    args = parser.parse_args()
    if platform.system() != 'Darwin':
        parser.error('This sampler uses macOS ps -M thread output.')
    binary = str(Path(args.binary).resolve())
    results = []
    for active, cancel_every, idle in [(0, 0, 0), (0, 0, 100), (1, 0, 100),
                                      (8, 0, 100), (32, 0, 100), (32, 4, 100)]:
        for repeat in range(args.repetitions):
            result = run(binary, active, cancel_every, idle)
            result['repeat'] = repeat + 1
            results.append(result)
            print(json.dumps(result), flush=True)
    report = {'timestamp': datetime.now(timezone.utc).isoformat(),
              'platform': platform.platform(), 'machine': platform.machine(),
              'binary': binary, 'profile': 'release',
              'limits': ['Synthetic injected host, 100 ms per inference, two tool calls.',
                         '100 idle sessions are retained suspended native GDK states.',
                         'RSS includes the binary and allocator, sampled externally with ps.',
                         'Checkpoint times measure JSON serialization only; no VFS, MCP, network or durable storage.',
                         'No old harness baseline or production capacity claim.'],
              'results': results}
    if args.output:
        Path(args.output).write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()
