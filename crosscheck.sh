#!/bin/sh
# crosscheck.sh — prove live API numbers against independent OS tools.
#
# Every check compares a /api/4 value with a second source that shares
# no code with the daemon (df, /proc, iproute2, ss, hwmon, nvidia-smi).
# Tolerances cover one 2s refresh tick of skew on a live system — not
# bugs. Prints one line per check, exits nonzero if any check fails.
#
# Usage:
#   ./crosscheck.sh [base-url]     # check a running deployment
#   ./crosscheck.sh                # build+start a scratch daemon on :61209
set -u

BASE="${1:-}"
SCRATCH=""

if [ -z "$BASE" ]; then
    ROOT=$(dirname "$0")
    BIN="$ROOT/target/debug/glances-rs"
    [ -x "$BIN" ] || { echo "build first: cargo build" >&2; exit 1; }
    BASE="http://localhost:61209"
    "$BIN" -w --web-port 61209 --disable-password >/tmp/crosscheck.log 2>&1 &
    DPID=$!
    SCRATCH="$DPID"
    trap 'kill $DPID 2>/dev/null' EXIT INT TERM
    for _ in $(seq 1 30); do
        if curl -s -m 2 -o /dev/null "$BASE/api/4/status"; then break; fi
        sleep 1
    done
    # Settle past the first tick (rates/percents need two samples).
    sleep 5
fi

command -v python3 >/dev/null 2>&1 || { echo "python3 is required" >&2; exit 1; }

python3 - "$BASE" <<'EOF'
import json, subprocess, sys, time, urllib.request

BASE = sys.argv[1]
fails, skips = [], []

def get(path):
    with urllib.request.urlopen(BASE + path, timeout=10) as r:
        return json.load(r)

def run(*cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        return None

def check(name, ok, detail=""):
    if ok is None:
        skips.append(name)
        print(f"SKIP - {name} ({detail})")
    elif ok:
        print(f"ok - {name} ({detail})" if detail else f"ok - {name}")
    else:
        fails.append(name)
        print(f"FAIL - {name} ({detail})")

def rel(a, b):
    return abs(a - b) / max(abs(b), 1e-9)

# ---- filesystems: df is the independent reader ----
try:
    out = run("df", "-B1", "/").stdout.splitlines()[-1].split()
    api = next(x for x in get("/api/4/fs") if x["mnt_point"] == "/")
    check("fs / size exact", api["size"] == int(out[1]), f"{api['size']}")
    check("fs / used within 0.5%", rel(api["used"], int(out[2])) < 0.005,
          f"api={api['used']} df={out[2]}")
except Exception as e:
    check("fs /", False, str(e))

# ---- memory: meminfo total never moves ----
try:
    mem = dict(l.split()[:2] for l in open("/proc/meminfo").readlines()[:3])
    total = int(mem["MemTotal:"]) * 1024
    api = get("/api/4/mem")
    check("mem total exact", api["total"] == total, f"{api['total']}")
except Exception as e:
    check("mem", False, str(e))

# ---- load + uptime: slow movers, tight bounds ----
try:
    # Bracket the API tick: it fired up to 2s before our fetch, while
    # load decays fast after bursty jobs — match either endpoint.
    def load_ok():
        la1 = list(map(float, open("/proc/loadavg").read().split()[:3]))
        av = [api["min1"], api["min5"], api["min15"]]
        la2 = list(map(float, open("/proc/loadavg").read().split()[:3]))
        return (all(min(abs(a - b1), abs(a - b2)) < 0.1
                    for a, b1, b2 in zip(av, la1, la2)), f"{la1}/{la2}")
    api = get("/api/4/load")
    ok, detail = load_ok()
    if not ok:
        # One retry: under request bursts the daemon tick can lag a
        # few seconds behind the kernel's 5s loadavg window.
        time.sleep(3)
        api = get("/api/4/load")
        ok, detail = load_ok()
    check("load 1/5/15 brackets API tick", ok, detail)
    up = float(open("/proc/uptime").read().split()[0])
    aus = get("/api/4/uptime")["seconds"]
    # 15s bound: tick staleness plus burst lag; still catches any
    # unit error (ms-vs-s would miss by 1000x).
    check("uptime fresh within 15s", 0 <= up - aus < 15, f"api={aus:.0f} kernel={up:.0f}")
except Exception as e:
    check("load/uptime", False, str(e))

# ---- ip: iproute2 + sysfs ----
try:
    r = run("ip", "-4", "route", "show", "default")
    api = get("/api/4/ip")
    if r and r.returncode == 0 and r.stdout.strip():
        toks = r.stdout.split()
        via = toks[toks.index("via") + 1] if "via" in toks else ""
        dev = toks[toks.index("dev") + 1] if "dev" in toks else ""
        mac = open(f"/sys/class/net/{dev}/address").read().strip() if dev else ""
        a = run("ip", "-4", "-o", "addr", "show", dev).stdout if dev else ""
        check("ip gateway exact", api["gateway"] == via, api["gateway"])
        check("ip mac exact", api["mac"].lower() == mac.lower(), api["mac"])
        check("ip address in iproute2", api["address"] and api["address"] in a, api["address"])
    else:
        check("ip", None, "no default route")
except Exception as e:
    check("ip", False, str(e))

# ---- network gauges: cumulative counters, allow tick skew ----
try:
    devs = {}
    for line in open("/proc/net/dev").readlines()[2:]:
        name, rest = line.split(":", 1)
        f = rest.split()
        devs[name.strip()] = (int(f[0]), int(f[8]))
    api = {x["interface_name"]: x for x in get("/api/4/network")}
    name = next((n for n in devs if n != "lo" and n in api), None)
    if name:
        krx, ktx = devs[name]
        arx = int(api[name]["bytes_recv_gauge"])
        atx = int(api[name]["bytes_sent_gauge"])
        check(f"net {name} gauges within 2%",
              rel(arx, krx) < 0.02 and rel(atx, ktx) < 0.02, f"rx {arx}/{krx}")
    else:
        check("net gauges", None, "no shared interface")
except Exception as e:
    check("net", False, str(e))

# ---- diskio: sectors*512, counts only move forward ----
try:
    ds = {}
    # fields: reads_completed[3] sectors_read[5] writes_completed[7] sectors_written[9]
    for line in open("/proc/diskstats").readlines():
        f = line.split()
        ds[f[2]] = (int(f[5]) * 512, int(f[9]) * 512, int(f[3]), int(f[7]))
    api = {x["disk_name"]: x for x in get("/api/4/diskio")}
    name = next((n for n in ds if n in api), None)
    if name:
        krb, kwb, krc, kwc = ds[name]
        a = api[name]
        check(f"diskio {name} read bytes within 0.5%", rel(a["read_bytes"], krb) < 0.005, "")
        check(f"diskio {name} counts sane",
              0 <= krc - a["read_count"] < 2000 and 0 <= kwc - a["write_count"] < 2000,
              f"rc {a['read_count']}/{krc}")
    else:
        check("diskio", None, "no shared disk")
except Exception as e:
    check("diskio", False, str(e))

# ---- processlist pid 1: stat/statm/cmdline ----
try:
    st = open("/proc/1/stat").read()
    tail = st[st.rfind(")") + 1:].split()
    kutime, kstime, kthr = int(tail[11]), int(tail[12]), int(tail[17])
    krss = int(open("/proc/1/statm").read().split()[1]) * 4096
    kcmd0 = open("/proc/1/cmdline", "rb").read().split(b"\0")[0].decode()
    p = next(x for x in get("/api/4/processlist") if x["pid"] == 1)
    check("pid1 cmdline exact", p["cmdline"][0] == kcmd0, kcmd0)
    aticks = p["cpu_times"]["user"] + p["cpu_times"]["system"]
    check("pid1 ticks sane", 0 <= kutime + kstime - aticks < 50, f"{aticks}/{kutime + kstime}")
    check("pid1 rss within 5%", rel(p["memory_info"]["rss"], krss) < 0.05, "")
    check("pid1 threads exact", p["num_threads"] == kthr, f"{kthr}")
except Exception as e:
    check("processlist pid1", False, str(e))

# ---- connections vs ss ----
try:
    r = run("ss", "-s")
    if r and r.returncode == 0:
        import re
        estab = int(re.search(r"estab (\d+)", r.stdout).group(1))
        tw = int(re.search(r"timewait (\d+)", r.stdout).group(1))
        api = get("/api/4/connections")
        check("connections established within 5", abs(api["ESTABLISHED"] - estab) <= 5,
              f"api={api['ESTABLISHED']} ss={estab}")
        # One-sided: this script's own curls create TIME_WAITs between
        # the daemon tick and our ss read, so api <= ss always. The
        # check is that the daemon invents nothing and misses little.
        check("connections timewait sane",
              api["TIME_WAIT"] <= tw + 10 and tw - api["TIME_WAIT"] < 60,
              f"api={api['TIME_WAIT']} ss={tw}")
    else:
        check("connections", None, "ss missing")
except Exception as e:
    check("connections", False, str(e))

# ---- cpu: independent 1s recompute vs daemon window ----
try:
    def rd():
        return list(map(int, open("/proc/stat").readline().split()[1:8]))
    a = rd()
    time.sleep(1)
    b = rd()
    mine = 100 * (1 - (b[3] + b[4] - a[3] - a[4]) / max(sum(b) - sum(a), 1))
    api = get("/api/4/cpu")
    busy = 100 - api["idle"]
    check("cpu busy within 6 points", abs(busy - mine) < 6, f"api={busy:.1f} mine={mine:.1f}")
except Exception as e:
    check("cpu", False, str(e))

# ---- hwmon + nvidia: conditional on hardware ----
try:
    import glob, os
    hit = False
    for inp in sorted(glob.glob("/sys/class/hwmon/hwmon*/temp*_input")):
        chip = open(os.path.join(os.path.dirname(inp), "name")).read().strip()
        lblp = inp.replace("_input", "_label")
        lbl = open(lblp).read().strip() if os.path.exists(lblp) else None
        kv = int(open(inp).read().strip()) / 1000.0
        api = [s for s in get("/api/4/sensors")
               if s["chip"] == chip and (lbl is None or s["label"] == lbl)]
        if api:
            hit = True
            check(f"sensor {chip}/{lbl} within 2C", abs(api[0]["value"] - kv) < 2.0,
                  f"api={api[0]['value']} sysfs={kv}")
            break
    if not hit:
        check("sensors", None, "no matchable hwmon temp")
except Exception as e:
    check("sensors", False, str(e))

try:
    r = run("nvidia-smi", "--query-gpu=temperature.gpu,utilization.gpu",
            "--format=csv,noheader,nounits")
    if r and r.returncode == 0 and r.stdout.strip():
        # Set-match: card order differs between sources, temps move.
        smi = sorted(float(l.split(",")[0]) for l in r.stdout.splitlines() if l.strip())
        apiv = sorted(g["temperature"] for g in get("/api/4/gpu")
                      if g["vendor"] == "nvidia" and g["temperature"] is not None)
        ok = len(smi) == len(apiv) and all(
            min(abs(a - s) for s in smi) < 5.0 for a in apiv)
        check("nvidia temps within 5C (set)", ok, f"api={apiv} smi={smi}")
    else:
        check("gpu", None, "nvidia-smi missing")
except Exception as e:
    check("gpu", False, str(e))

print(f"\n{len(fails)} failed, {len(skips)} skipped")
sys.exit(1 if fails else 0)
EOF
