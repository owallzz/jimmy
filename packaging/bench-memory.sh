#!/usr/bin/env bash
# Rough memory/CPU benchmark on Linux: plays a file under Xvfb and samples /proc.
#   packaging/bench-memory.sh /path/to/jimmy /path/to/movie.mkv [seconds]
set -euo pipefail
BIN="$1"; MOVIE="$2"; SECS="${3:-40}"
export DISPLAY=:99 LIBGL_ALWAYS_SOFTWARE=1
unset WAYLAND_DISPLAY
pgrep -x Xvfb >/dev/null || { Xvfb :99 -screen 0 1920x1080x24 >/dev/null 2>&1 & sleep 1; }
H="$(mktemp -d)"
printf 'roots = ["%s"]\n%b\n' "$(dirname "$MOVIE")" "${JIMMY_BENCH_CONFIG:-}" > "$H/config.toml"
ARG="$MOVIE"; [ "$(basename "$MOVIE")" = "-" ] && ARG="$(dirname "$MOVIE")"   # dir/- = idle in the browser
JIMMY_HOME="$H" "$BIN" "$ARG" --windowed --size 1920x1080 >/dev/null 2>&1 &
P=$!
ticks=$(getconf CLK_TCK)
cpu() { awk '{print $14+$15}' /proc/$P/stat; }
sleep 3
c0=$(cpu); t0=$(date +%s.%N)
printf '%6s %9s %9s %9s %9s %6s\n' "t(s)" "RSS(MB)" "anon(MB)" "file(MB)" "PssMB" "cpu%"
for t in $(seq 5 5 "$SECS"); do
  sleep 5
  c1=$(cpu); t1=$(date +%s.%N)
  rss=$(awk '/^Rss:/{print $2}' /proc/$P/smaps_rollup)
  anon=$(awk '/^Anonymous:/{print $2}' /proc/$P/smaps_rollup)
  pss=$(awk '/^Pss:/{print $2}' /proc/$P/smaps_rollup)
  file=$(( rss - anon ))
  pct=$(awk -v a="$c0" -v b="$c1" -v x="$t0" -v y="$t1" -v k="$ticks" 'BEGIN{printf "%.0f", (b-a)/k/(y-x)*100}')
  printf '%6s %9.0f %9.0f %9.0f %9.0f %6s\n' "$t" "$((rss/1024))" "$((anon/1024))" "$((file/1024))" "$((pss/1024))" "$pct"
  c0=$c1; t0=$t1
done
kill $P 2>/dev/null; wait $P 2>/dev/null || true
rm -rf "$H"
