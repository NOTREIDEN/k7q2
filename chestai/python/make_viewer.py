"""The replay page: episodes written by watch.py, each with a label.

    python make_viewer.py out.html "Trained, full:ep1.json" "Teacher, full:ep2.json"
"""
import json
import os
import sys


def main():
    out = sys.argv[1]
    eps = []
    for arg in sys.argv[2:]:
        label, path = arg.rsplit(":", 1)
        e = json.load(open(path))
        e["label"] = label
        eps.append(e)
    tpl = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "viewer_template.html")).read()
    with open(out, "w") as f:
        f.write(tpl.replace("__EPISODES__", json.dumps(eps, separators=(",", ":"))))
    print(out, os.path.getsize(out) // 1024, "KB")


if __name__ == "__main__":
    main()
