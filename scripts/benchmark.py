#!/usr/bin/env python3
"""Measure an existing scratch Roblox installation without changing Roseline's bottle.

Uses only the Python standard library. Restores the scratch client's settings and
stops only the process group it starts. Refuses to share an existing muvm VM.
The reported gap statistics describe per-interval maxima, not frame percentiles.
"""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import signal
import statistics
import subprocess
import time


def muvm_running():
    for path in Path('/proc').glob('[0-9]*/cmdline'):
        try:
            program = path.read_bytes().split(b'\0', 1)[0]
        except OSError:
            continue
        if program and os.path.basename(program) == b'muvm':
            return True
    return False


def counters(line):
    return {key: int(value) for key, value in re.findall(r'(\w+)=(\d+)(?:/s)?', line)}


def summarize(samples):
    frames = sum(row.get('frames', 0) for row in samples)
    gaps = [row['frame_gap_micros_peak'] / 1000 for row in samples
            if 'frame_gap_micros_peak' in row]
    return {
        'intervals': len(samples),
        'fps_mean': round(frames / len(samples), 2),
        'draws_per_frame': round(sum(row.get('draws', 0) for row in samples) / frames, 1)
        if frames else None,
        'interval_peak_gap_median_ms': round(statistics.median(gaps), 2) if gaps else None,
        'worst_gap_ms': round(max(gaps), 2) if gaps else None,
        'intervals_with_gap_over_33ms': sum(gap > 33.333 for gap in gaps),
        'gpu_time_ms_per_interval': round(statistics.mean(row.get('gpu_micros', 0)
                                                        for row in samples) / 1000, 2),
    }


def stop(process, unit=None):
    if unit:
        subprocess.run(['systemctl', '--user', 'stop', unit],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15)
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=10)


def interrupted(_signum, _frame):
    raise KeyboardInterrupt


def scope_memory(unit):
    result = subprocess.run(['systemctl', '--user', 'show', unit,
                             '-p', 'MemoryPeak', '-p', 'MemorySwapPeak', '-p', 'ControlGroup'],
                            capture_output=True, text=True, timeout=5)
    properties = dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)
    memory = {}
    for field in ('MemoryPeak', 'MemorySwapPeak'):
        value = properties.get(field, '')
        if value.isdigit():
            memory[field] = round(int(value) / 1024**2, 1)
    group = properties.get('ControlGroup')
    if group:
        events = Path('/sys/fs/cgroup') / group.lstrip('/') / 'memory.events'
        try:
            memory['events'] = {key: int(value) for key, value
                                in (line.split() for line in events.read_text().splitlines())}
        except OSError:
            pass
    return memory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True, help='scratch root containing app/ and prefix/')
    parser.add_argument('--runner', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True, help='new directory for logs and summary')
    parser.add_argument('--vm', action='store_true')
    parser.add_argument('--vm-memory-mib', type=int,
                        help='optional guest RAM limit, distinct from the host test scope limit')
    parser.add_argument('--quality', type=int, choices=range(11), default=10)
    parser.add_argument('--fps-cap', type=int, choices=(60, 120, 144, 240), default=240)
    parser.add_argument('--fullscreen', action='store_true')
    parser.add_argument('--place', default='5771467270')
    parser.add_argument('--job-id', help='reuse the same public server between runs')
    parser.add_argument('--warmup', type=float, default=30)
    parser.add_argument('--duration', type=float, default=45)
    parser.add_argument('--env', action='append', default=[], metavar='NAME=VALUE')
    parser.add_argument('--memory-limit-mib', type=int, default=4096,
                        help='isolated systemd scope memory limit (default: 4096 MiB)')
    args = parser.parse_args()
    if args.duration < 5 or args.warmup < 0:
        parser.error('duration must be at least 5 seconds and warmup nonnegative')
    if args.memory_limit_mib < 256:
        parser.error('memory limit must be at least 256 MiB')
    if args.vm_memory_mib is not None and (not args.vm or args.vm_memory_mib < 256):
        parser.error('guest RAM limit requires --vm and at least 256 MiB')
    if not shutil.which('systemd-run') or not shutil.which('systemctl'):
        parser.error('systemd-run and a user systemd session are required for memory isolation')
    args.root = args.root.resolve()
    args.runner = args.runner.resolve()
    args.output = args.output.resolve()
    if args.vm and muvm_running():
        parser.error('an existing muvm VM must be closed before benchmarking')
    files = list((args.root / 'prefix/root/Users').glob('*/Library/Roblox/GlobalBasicSettings_*.xml'))
    if len(files) != 1:
        parser.error('expected exactly one scratch Roblox settings file')
    original = files[0].read_bytes()
    changed = original.decode()
    for field, value in {
        'SavedQualityLevel': args.quality,
        'GraphicsQualityLevel': args.quality * 2 + 1,
        'FramerateCap': args.fps_cap,
        'Fullscreen': str(args.fullscreen).lower(),
    }.items():
        changed, count = re.subn(r'(<(?:int|token|bool) name="' + field + r'">)[^<]+',
                                 lambda match: match[1] + str(value), changed)
        if count != 1:
            parser.error(f'expected exactly one {field} setting')
    environment = {
        'ROSEMARY_METAL_STATS': '1',
        'ROSEMARY_PREFIX': str(args.root / 'prefix'),
        'ROSEMARY_PREFIX_HOME': 'prefix',
        'ROSEMARY_PREFIX_SETUP': 'download',
        'ROSEMARY_DECORATIONS': 'server',
        'ROSEMARY_MENU': 'none',
    }
    for line in args.env:
        name, separator, value = line.partition('=')
        if not separator or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', name):
            parser.error('environment must be NAME=VALUE')
        environment[name] = value
    args.output.mkdir(parents=True, exist_ok=False)
    (args.output / 'settings.original.xml').write_bytes(original)
    url = f'roblox://experiences/start?placeId={args.place}'
    if args.job_id:
        url += f'&gameInstanceId={args.job_id}'
    command = [str(args.runner), 'run', str(args.root / 'app/RobloxPlayer.app')]
    for flag in ('-protocolString', '-protocolUrl', '-protocolUri'):
        command.extend((flag, url))
    command.extend((url, '-placeId', args.place))
    if args.job_id:
        command.extend(('-gameInstanceId', args.job_id))
    if args.vm:
        pre = args.output / 'overcommit.sh'
        pre.write_text('#!/bin/sh\nset -eu\necho 1 > /proc/sys/vm/overcommit_memory\n')
        pre.chmod(0o755)
        vm_options = [] if args.vm_memory_mib is None else [f'--mem={args.vm_memory_mib}']
        command = ['muvm', *vm_options, '-x', str(pre)] + [item for pair in environment.items()
                    for item in ('-e', '='.join(pair))] + [
            '--', '/bin/sh', '-c',
            'exec >>"$2" 2>&1; cd "$1" || exit; shift 2; exec "$@"',
            'sh', str(args.root / 'app'), str(args.output / 'game.log'), *command]
    unit = f'roseline-bench-{os.getpid()}.scope'
    command = ['systemd-run', '--user', '--scope', '--quiet', '--collect', '--unit', unit,
               '-p', f'MemoryMax={args.memory_limit_mib}M',
               '-p', 'MemorySwapMax=256M', '--', *command]
    process = None
    samples = []
    joined_at = None
    server = None
    framebuffer = None
    measured_sizes = set()
    deadline = time.monotonic() + 150
    signal.signal(signal.SIGTERM, interrupted)
    try:
        files[0].write_text(changed)
        with (args.output / 'game.log').open('a') as log:
            process = subprocess.Popen(command, cwd=args.root / 'app',
                                       env=os.environ | environment,
                                       stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                       start_new_session=True)
        with (args.output / 'game.log').open() as reader:
            while True:
                line = reader.readline()
                now = time.monotonic()
                if line:
                    if 'GL Renderer:' in line:
                        raise RuntimeError('client fell back to OpenGL; Metal counters cannot measure this run')
                    size = re.search(r'Metal: (?:creating framebuffer|resizing main framebuffer to) (\d+)x(\d+)', line)
                    if size:
                        framebuffer = tuple(map(int, size.groups()))
                    match = re.search(r"Joining game '([a-f0-9-]+)'", line)
                    if match:
                        server = match[1]
                    if 'Replicator created:' in line and joined_at is None:
                        joined_at = now
                        print('Joined; warming up.', flush=True)
                    if joined_at and now >= joined_at + args.warmup and 'Metal: stats ' in line:
                        samples.append(counters(line))
                        if framebuffer:
                            measured_sizes.add(framebuffer)
                elif process.poll() is not None:
                    raise RuntimeError(f'runner exited early ({process.returncode}); see game.log')
                else:
                    time.sleep(0.1)
                if joined_at and now >= joined_at + args.warmup + args.duration:
                    break
                if joined_at is None and now > deadline:
                    raise RuntimeError('no game join within 150 seconds; see game.log')
        if len(samples) < args.duration * 0.7:
            raise RuntimeError('insufficient rendering samples; see game.log')
        result = {'quality': args.quality, 'fps_cap': args.fps_cap,
                  'fullscreen': args.fullscreen, 'server': server,
                  'vm_memory_mib': args.vm_memory_mib,
                  'scope_memory_mib': scope_memory(unit),
                  'framebuffer_sizes': sorted(measured_sizes),
                  'environment': environment, **summarize(samples)}
        (args.output / 'summary.json').write_text(json.dumps(result, indent=2) + '\n')
        (args.output / 'samples.json').write_text(json.dumps(samples) + '\n')
        print(json.dumps(result, indent=2), flush=True)
    finally:
        try:
            if process is not None:
                stop(process, unit)
        finally:
            files[0].write_bytes(original)


if __name__ == '__main__':
    main()
