"""Print trace lines with hex floats decoded: dec.py TRACE 'awk condition'"""
import sys, struct, re, subprocess
def d(h): return struct.unpack('>d', bytes.fromhex(h.zfill(16)))[0]
def f(h): return struct.unpack('>f', bytes.fromhex(h.zfill(8)))[0]
D = {'x','y','z','mx','my','mz','attr','rx','ry','rz','rtx','rty','rtz','lrx','lry','lrz','tmx','tmy','tmz'}
F = {'yaw','pitch','hyaw','jmf','fall','ld','hp','ryaw','rpitch','lryaw','lrpitch','mf','ms','mif','mis'}
out = subprocess.run(['awk', sys.argv[2], sys.argv[1]], capture_output=True, text=True).stdout
keep = set(sys.argv[3].split(',')) if len(sys.argv) > 3 else None
for line in out.splitlines():
    w = line.split()
    res = []
    for t in w:
        if '=' in t:
            k, v = t.split('=', 1)
            if keep and k not in keep: continue
            try:
                if k in D and not line.startswith(('SS','CP')) : t = '%s=%.6f' % (k, d(v))
                elif k in D and re.fullmatch(r'[0-9a-f]{9,16}', v): t = '%s=%.6f' % (k, d(v))
                elif k in F and 'S14' not in line and 'S18' not in line and 'S19' not in line: t = '%s=%.3f' % (k, f(v))
            except Exception: pass
        res.append(t)
    print(' '.join(res))
