"""Vendor official MIT shadcn-vue registry sources into the Vue desktop app.

Registry responses are data: only .vue/.ts UI files are written, and no registry
scripts or install commands are executed. Exact response hashes are recorded.
"""
import hashlib
import json
from pathlib import Path
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1] / "desktop"
STYLE = "new-york-v4"
pending = "sidebar card button badge input textarea tabs progress tooltip dialog separator table".split()
seen = set()
manifest = []
dependencies = set()
while pending:
    name = pending.pop(0)
    if name in seen:
        continue
    if not name.replace("-", "").isalnum():
        raise ValueError("Unexpected registry dependency")
    seen.add(name)
    url = f"https://www.shadcn-vue.com/r/styles/{STYLE}/{name}.json"
    with urlopen(Request(url, headers={"User-Agent": "LibraryWorkspace/0.1"}), timeout=30) as response:
        raw = response.read(2 * 1024 * 1024 + 1)
    if len(raw) > 2 * 1024 * 1024:
        raise ValueError("Oversized registry response")
    item = json.loads(raw)
    if item["name"] != name or item["type"] != "registry:ui":
        raise ValueError("Not a UI registry item")
    for file in item["files"]:
        original = Path(file["path"])
        prefix = f"registry/{STYLE}/ui/"
        relative = file["path"].replace("\\", "/")
        if not relative.startswith(prefix) or ".." in original.parts or original.suffix not in {".vue", ".ts"}:
            raise ValueError("Unsupported registry path")
        target = ROOT / "src/components/ui" / relative[len(prefix):]
        target.parent.mkdir(parents=True, exist_ok=True)
        content = file["content"].replace(f"@/registry/{STYLE}/ui/", "@/components/ui/")
        target.write_text(content, encoding="utf-8")
    pending.extend(item.get("registryDependencies", []))
    dependencies.update(item.get("dependencies", []))
    manifest.append({"component": name, "url": url, "sha256": hashlib.sha256(raw).hexdigest()})
    print(f"Vendored {name}", flush=True)
(ROOT / "licenses/shadcn-registry.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
print("Dependencies:", " ".join(sorted(dependencies)))
