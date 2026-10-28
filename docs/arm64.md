# ARM64 builds and runtime testing

Roséline itself builds natively on ARM64. A Rosemary runner is a separate
component: prefer an upstream ARM64 release when one exists; otherwise the
native Roséline package can run the upstream x86_64 runner through muvm/FEX.
On Asahi, muvm also supplies the 4 KiB pages the x86 runtime needs.

Settings → Advanced → ARM64 compatibility enables that fallback by default
on ARM64. Turning it off requires a native runner. The setting persists as
`[compatibility] muvm = false` in `settings.toml`. It does not change native
runner execution. Flatpak cannot use this fallback.

The same settings group offers a VM memory selector: muvm's default, 4 GiB,
6 GiB or 8 GiB. The default leaves muvm's sizing unchanged. A selected size
applies when starting a new VM and does not alter Roblox graphics quality.

On Fedora Asahi, install runtime dependencies with:

```sh
sudo dnf install muvm fex-emu
```

Close other muvm applications before starting Roblox: muvm otherwise joins
the existing VM, bypassing the startup configuration and process monitoring.
Startup diagnostics and guest output are kept in the bottle's logs directory.

## Verification

```sh
cargo test
cargo build --release --locked
cargo fmt --check
shellcheck scripts/setup.sh build-aux/make-*.sh
actionlint .github/workflows/release.yml
```

The additional VM integration test must run alone, outside Flatpak, on a host
with muvm/FEX installed and no existing VM:

```sh
cargo test vm_preserves_arguments_environment_logs_and_exit_status -- --ignored
```

## Performance measurements

Use a scratch installation containing `app/` and `prefix/`; keep it separate
from Roséline's managed bottle. Example with an existing x86_64 runner:

```sh
python3 scripts/benchmark.py \
  --root /absolute/path/to/scratch \
  --runner /absolute/path/to/Rosemary/bin/rosemary \
  --output /absolute/path/to/new-results-directory \
  --vm --quality 10 --warmup 30 --duration 45 \
  --memory-limit-mib 5120 --vm-memory-mib 4096
```

For repeat runs, `--job-id` reuses the public server ID from `summary.json`.
Match quality, resolution, camera and server population before attributing a
difference to a setting. A fixed server reduces variation but does not freeze
the scene. Stop compilers and other benchmarks before measuring.

The script preserves settings, stops only the process group it starts, and
fails when a client exits early or produces insufficient samples. Outputs
include raw logs, samples and a JSON summary. Keep raw logs local; client logs
can contain account and session information.

Tests run in their own user systemd scope, limited to 4 GiB RAM and 256 MiB
swap by default. `--memory-limit-mib` adjusts the RAM limit. Stopping a test
also stops its scope, including detached runner helpers. The original settings
are saved as `settings.original.xml` before launch so an interrupted test can
be recovered even if its controller is killed.

The example allows 5 GiB on the host and 4 GiB inside the VM; leave enough
available host memory for that budget. The summary records peak scope memory
and cgroup memory events. A run that hits its scope limit is unsuitable for
attributing performance differences to another setting. Graphics quality 10
is the optimization target; lower-quality runs are diagnostic references only.

Rosemary reports frame rates and each reporting interval's largest frame gap.
The median of those maxima is **not** a frame-time percentile or a 1% low.
GPU time counters also do not directly measure system-wide GPU utilization.
