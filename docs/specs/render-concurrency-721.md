# Render concurrency measurements (#721)

This is a diagnostic of the direct `audio_engine` renderer at commit
`2049e4069a264d5e99e0da9a6a349bf1491acf40`. It is not an application-level
benchmark: it does not include preview or agent dispatch, application state,
or other processes. The excluded `docs/specs/` directory is not published by
the website (`.mintignore`).

## Method

The ignored `render_concurrency_measure` integration test starts one, two or
four worker threads against the direct `Engine::render_to_wav` entry point.
Each worker writes and checks a distinct WAV. A barrier synchronizes entry;
each configuration runs in a fresh process. Same-project workers share a
session and its input media; different-project workers use separate sessions
and independently created files with identical samples. Output checks include
dimensions, non-silence, limiter ceiling and matching hashes. Timings exclude
input creation and output verification. Process `VmHWM` includes setup and
all worker threads. Linux documents `/proc` RSS as approximate, and it does
not include all buffered file-cache memory.

Environment: Linux x86-64 6.18.55, AMD EPYC 9V45, Rust 1.90.0, release profile,
overlay filesystem, four CPU cores selected using `taskset` with a four-CPU
shared cgroup quota. The shared cgroup has a 32 GiB memory limit; it was also
used by other activity. Each benchmark process had a 3 GiB virtual-address
limit, a 16,384 descriptor limit and a disk-space preflight. File cache was
not dropped. Source creation immediately precedes rendering, so media is
likely warm; cache and other workloads were not controlled. A shared-cgroup
headroom check is a precaution, not an isolated memory budget.

Workloads:

- **Limiter:** 600 seconds of 48 kHz stereo, with a master limiter at -20 dBFS.
  The non-bypassed limiter prevents the single-clip unity-copy fast path.
- **Mixed:** 180 seconds, 30 stereo tracks (15 each at 44.1 and 48 kHz),
  rendered at 48 kHz with the same master limiter. Tracks in a project reuse
  one source per rate, not 30 independent media files.
- **Huge:** a renderable approximation of `huge_state` in
  `crates/ai/tests/request_size.rs`: 200 tracks, 50 buses, 150 sends,
  1,000 annotations and 184.5 seconds at 48 kHz. The original source paths
  do not exist. This fixture substitutes one real shared stereo source per
  project, sets an explicit session length and configures the limiter at
  -20 dBFS instead of the original default. The renderer reports all 200
  compressors, 200 EQs and 50 reverbs as skipped; these effects are not
  measured. Distinct track media and their cache costs are not represented.

## Results

Two fresh-process runs per configuration; the ranges are the two observed
values, not confidence intervals. Seconds are elapsed wall time for the whole
batch, not the sum of worker durations. MiB is the process high-water RSS.

| Workload | Projects | Workers | Batch seconds | Peak RSS MiB |
| --- | --- | ---: | ---: | ---: |
| Limiter | Same | 1 | 0.89–0.91 | 4.02–4.23 |
| Limiter | Same | 2 | 0.90–0.91 | 5.86–6.00 |
| Limiter | Same | 4 | 0.90–1.01 | 9.65–9.93 |
| Limiter | Different | 1 | 0.88–0.91 | 4.03–4.32 |
| Limiter | Different | 2 | 0.89–0.91 | 5.89–6.02 |
| Limiter | Different | 4 | 0.92–0.94 | 9.60–9.65 |
| Mixed | Same | 1 | 6.25–6.25 | 22.86–23.18 |
| Mixed | Same | 2 | 6.24–6.38 | 43.34–43.51 |
| Mixed | Same | 4 | 6.41–6.42 | 83.82–83.98 |
| Mixed | Different | 1 | 6.23–6.27 | 23.18–23.25 |
| Mixed | Different | 2 | 6.32–6.41 | 43.21–43.43 |
| Mixed | Different | 4 | 6.34–6.48 | 84.08–84.09 |
| Huge | Same | 1 | 36.39–36.49 | 170.54–170.84 |
| Huge | Same | 2 | 36.83–37.03 | 338.73–339.05 |
| Huge | Same | 4 | 37.24–37.45 | 675.32–675.37 |
| Huge | Different | 1 | 36.26–36.31 | 170.77–170.90 |
| Huge | Different | 2 | 36.82–36.94 | 339.21–339.26 |
| Huge | Different | 4 | 37.12–37.38 | 675.76–676.22 |

The large fixture's peak process RSS rose nearly in proportion to concurrent
render count, even for the same project. Its four-worker batches took about
as long as one worker on this machine, so parallelism increased throughput
while consuming substantially more resident memory. In all four large,
four-worker runs, the sum of individual worker durations exceeded three times
batch duration by at least 36.6 seconds; because all measured intervals lie
within the batch, this bounds the time all four calls were in flight. It does
not measure scheduling latency outside those intervals.

## Recommendation and limitations

**Keep the existing mutex for now.** Removing serialization or creating an
engine per project can admit multiple memory-intensive renders; the measured
process memory growth is material, and neither a product memory budget nor a
representative end-to-end application measurement is established here. This
does not prove the current mutex bounds all render memory: some existing paths
bypass it. Nor does it establish a safe concurrency limit for any other machine.

After #714 provides a common render path, measure the actual preview and agent
paths with representative independent media and isolated process/cgroup memory
accounting, including cache and competing application work. Set and validate an
explicit memory/concurrency budget before deciding whether a bounded scheduler
can replace or narrow the mutex. Two samples on one shared machine are not a
substitute for a workload distribution or a latency-tail study.

To reproduce a point on Linux, build in release mode, give it sufficient disk,
memory and file descriptors, and run in a fresh process (remove `BENCH_SECONDS`
if set):

```sh
BENCH_WORKLOAD=huge BENCH_SCOPE=different BENCH_WORKERS=4 \
  cargo test -p audio-engine --release --test render_concurrency_measure \
  -- --ignored --exact measure_render_concurrency --nocapture
```

The test header lists all configurations and the short smoke-run override.
On this host the measurements used `taskset -c 0-3` and `ulimit -v 3145728`;
CPU numbering and a safe limit depend on the target system.

## References

- [ADR 0003](../adr/0003-rendering-a-node.md) retains the mutex pending evidence.
- [Linux `proc_pid_status(5)`](https://man7.org/linux/man-pages/man5/proc_pid_status.5.html)
  describes `VmRSS` and `VmHWM` and their accuracy limitations.
- [Linux cgroup v2 memory documentation](https://docs.kernel.org/admin-guide/cgroup-v2.html#memory-interface-files)
  describes a different accounting scope that includes file cache.
- [Rust `Instant`](https://doc.rust-lang.org/std/time/struct.Instant.html) is
  used for monotonic elapsed time.
