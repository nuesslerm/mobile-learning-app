"""Build the offline benchmark catalog from validated, repository-local records."""

from collections import defaultdict
from datetime import date
from html import escape
from pathlib import Path
from statistics import median
from urllib.parse import quote
import json
import math
import os
import re
import sys
import tempfile

ROOT = Path.cwd()
LIB = ROOT / "docs/benchmarks"


def encode(value):
    return json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n"


def read(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key {key}")
            result[key] = value
        return result

    return json.loads(path.read_text(), object_pairs_hook=unique)


def validate(value, rule, path):
    # Implements the keywords used by the public draft 2020-12 schema.
    kind = {
        str: "string",
        int: "number",
        float: "number",
        list: "array",
        dict: "object",
        type(None): "null",
        bool: "boolean",
    }[type(value)]
    types = rule.get("type", [kind])
    if isinstance(types, str):
        types = [types]
    if kind not in types or ("enum" in rule and value not in rule["enum"]):
        raise ValueError(f"{path}: unexpected value {value!r}")
    if "const" in rule and (value != rule["const"] or isinstance(value, bool)):
        raise ValueError(f"{path}: expected {rule['const']}")
    if kind == "object":
        fields = rule.get("properties", {})
        unknown = (
            value.keys() - fields.keys()
            if rule.get("additionalProperties") is False
            else set()
        )
        missing = set(rule.get("required", [])) - value.keys()
        if missing or unknown:
            raise ValueError(
                f"{path}: missing {sorted(missing)}, unknown {sorted(unknown)}"
            )
        for key, subrule in fields.items():
            if key in value:
                validate(value[key], subrule, f"{path}.{key}")
    if kind == "array":
        for i, child in enumerate(value):
            validate(child, rule["items"], f"{path}[{i}]")
    if kind == "string" and (
        len(value) < rule.get("minLength", 0)
        or not re.search(rule.get("pattern", ""), value)
    ):
        raise ValueError(f"{path}: invalid string")
    if kind == "number" and not math.isfinite(value):
        raise ValueError(f"{path}: expected a finite number")


def load_report(path, schema):
    r = read(path)
    validate(r, schema, "report")
    if r["id"] != path.parent.name:
        raise ValueError("directory must match report ID")
    if r["date"]:
        date.fromisoformat(r["date"])
    refs = {}
    for name in ("variants", "scenarios", "metrics"):
        ids = [item["id"] for item in r[name]]
        if len(set(ids)) != len(ids):
            raise ValueError(f"duplicate ID in {name}")
        refs[name] = set(ids)
    seen = set()
    for o in r["observations"]:
        for key, collection in (
            ("variant", "variants"),
            ("scenario", "scenarios"),
            ("metric", "metrics"),
        ):
            if o[key] not in refs[collection]:
                raise ValueError(f"unknown {key} {o[key]}")
        identity = tuple(
            o[k] for k in ("variant", "scenario", "metric", "sample", "statistic")
        )
        if identity in seen:
            raise ValueError(f"duplicate observation {identity}")
        seen.add(identity)
        if o["outcome"] == "observed" and o["value"] is None:
            raise ValueError("observed value is missing")
    for t in r["tables"]:
        if not t["headers"] or any(len(row) != len(t["headers"]) for row in t["rows"]):
            raise ValueError("historical table column count mismatch")
    if not r["evidence"]:
        raise ValueError("at least one evidence reference required")
    for link in r["evidence"]:
        target = (path.parent / link["path"]).resolve()
        if (
            Path(link["path"]).is_absolute()
            or not target.is_relative_to(ROOT)
            or not target.is_file()
        ):
            raise ValueError(f"invalid repository evidence {link['path']}")
    return r


def e(value):
    return escape(str(value if value is not None else "Unreported"), quote=True)


def page(title, nav, body, css):
    return (
        f'<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" '
        f'content="width=device-width,initial-scale=1"><title>{e(title)}</title><style>{css}</style>'
        f'<nav aria-label="Report navigation">{nav}</nav><main>{body}</main></html>\n'
    )


def table(headers, rows):
    head = "".join(f'<th scope="col">{e(x)}</th>' for x in headers)
    body = "".join(
        "<tr>" + "".join(f"<td>{e(x)}</td>" for x in row) + "</tr>" for row in rows
    )
    return f'<div class="table-wrap" tabindex="0"><table><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table></div>'


def paired(r):
    variants = r["variants"]
    if len(variants) > 2:
        sections = []
        for scenario in r["scenarios"]:
            rows = [o for o in r["observations"] if o["scenario"] == scenario["id"]]
            participating = {o["variant"] for o in rows}
            group = [v for v in variants if v["id"] in participating]
            if len(group) == 2:
                sections.append(paired(dict(r, variants=group, scenarios=[scenario], observations=rows)))
        return "".join(sections)
    if len(variants) < 2:
        return ""
    a, b = [v["id"] for v in variants]
    groups = defaultdict(lambda: defaultdict(dict))
    for o in r["observations"]:
        if (
            o["statistic"] == "raw"
            and o["outcome"] == "observed"
            and type(o["value"]) in (float, int)
        ):
            groups[o["scenario"], o["metric"]][o["variant"]][o["sample"]] = o["value"]
    cards, charts, summaries = [], [], []
    for scenario in r["scenarios"]:
        for m in r["metrics"]:
            data = groups[scenario["id"], m["id"]]
            pairs = sorted(data[a].keys() & data[b].keys())
            if not pairs:
                continue
            left, right = [[data[v][p] for p in pairs] for v in (a, b)]
            med = [median(left), median(right)]
            delta = median([y - x for x, y in zip(left, right)])
            title = f"{scenario['label']} · {m['label']}"
            ratios = [(y - x) * 100 / x for x, y in zip(left, right) if x > 0]
            percent = (
                f"{median(ratios):+.1f}%"
                if len(ratios) == len(pairs)
                else "Unavailable"
            )
            summaries.append(
                [
                    title,
                    m["unit"],
                    str(len(pairs)),
                    f"{med[0]:.2f} ({min(left):.2f}–{max(left):.2f})",
                    f"{med[1]:.2f} ({min(right):.2f}–{max(right):.2f})",
                    f"{delta:+.2f}",
                    percent,
                ]
            )
            cards.append(
                f'<div class="card"><small>{e(title)}</small><p><b>{delta:+.2f} {e(m["unit"])}</b></p>'
                f"<small>Median paired difference · {e(b)} minus {e(a)}</small><p>{e(a)} {med[0]:.2f} · {e(b)} {med[1]:.2f}</p></div>"
            )
            upper = max(left + right)
            lower = min(left + right)

            def x(n):
                return 105 + 345 * (n - lower) / (upper - lower or 1)

            rows = []
            for i, p in enumerate(pairs):
                y = 25 + i * 26
                xa, xb = x(left[i]), x(right[i])
                rows.append(
                    f'<text x="4" y="{y + 4}">{e(p)}</text><path d="M{xa:.1f},{y} H{xb:.1f}" stroke="#859991"/>'
                    f'<circle cx="{xa:.1f}" cy="{y}" r="5" fill="#006653"/><rect x="{xb - 4:.1f}" y="{y - 4}" width="8" height="8" fill="#ad501c"/>'
                )
            charts.append(
                f'<h3>{e(title)}</h3><p class="meta">● {e(a)} · ■ {e(b)} · scale {lower:.2f}–{upper:.2f} {e(m["unit"])}</p>'
                f'<svg class="chart" role="img" aria-label="{e(title)} by pair; values in table below" viewBox="0 0 465 {35 + len(pairs) * 26}">{"".join(rows)}</svg>'
            )
    if not cards:
        return ""
    return (
        '<div class="cards">'
        + "".join(cards[:3])
        + "</div><h2>Paired measurements</h2>"
        + table(
            [
                "Metric",
                "Unit",
                "Pairs",
                variants[0]["label"] + " median (range)",
                variants[1]["label"] + " median (range)",
                "Median paired change",
                "Median paired change (%)",
            ],
            summaries,
        )
        + "".join(charts)
    )


def report(r, css):
    body = (
        f'<header><div class="meta">{e(r["subject"])} · {e(r["date"])}</div><h1>{e(r["title"])}</h1>'
        f'<p>{e(r["question"])}</p><p class="finding">{e(r["finding"])}</p></header>'
    ) + paired(r)
    if r["observations"]:
        metrics = {m["id"]: m for m in r["metrics"]}
        body += "<h2>Measurements</h2>" + table(
            [
                "Scenario",
                "Variant",
                "Sample",
                "Metric",
                "Statistic",
                "Value",
                "Unit",
                "Outcome",
            ],
            (
                [
                    o["scenario"],
                    o["variant"],
                    o["sample"],
                    metrics[o["metric"]]["label"],
                    o["statistic"],
                    o["value"],
                    metrics[o["metric"]]["unit"],
                    o["outcome"],
                ]
                for o in r["observations"]
            ),
        )
    for t in r["tables"]:
        body += table(t["headers"], t["rows"])
    body += "<h2>Method and provenance</h2>"
    body += f"<h3>Sample size</h3><p>{e(r.get('sampleSize'))}</p>"
    for k in ("workload", "method", "environment", "revision"):
        body += f'<h3>{k.capitalize()}</h3><p class="method">{e(r[k])}</p>'
    body += (
        "<h2>Evidence</h2><ul>"
        + "".join(
            f'<li><a href="{e(quote(item["path"]))}">{e(item["label"])}</a></li>'
            for item in r["evidence"]
        )
        + "</ul>"
    )
    return page(
        r["title"],
        '<a href="../../index.html">Benchmark library</a><a href="report.json">Report JSON</a>',
        body,
        css,
    )


def catalog(records, css):
    body = '<header><h1>Benchmark library</h1></header><label for="q">Search reports</label><input type="search" id="q">'
    md = [
        "# Benchmark library",
        "",
        "[Offline browser catalog](index.html) · [Machine-readable catalog](index.json) · [Report schema](schema/report-v1.schema.json) · [Historical source inventory](migration-inventory.json)",
        "",
        "Add a directory under `reports/` whose name matches the record ID. Write `report.json` against the schema and link the original evidence. Use the same sample ID for both measurements in a pair. Record failures and unavailable values explicitly. Run `wf bench reports build` and commit the generated views alongside the record. Run `wf bench reports check` to verify reproducibility.",
        "",
    ]
    for subject in dict.fromkeys(r["subject"] for r in records):
        body += f"<section><h2>{e(subject)}</h2>"
        md += ["## " + subject, ""]
        for r in (r for r in records if r["subject"] == subject):
            path = "reports/" + r["id"] + "/"
            variants = " · ".join(v["label"] for v in r["variants"])
            body += (
                f'<article><small>{e(r["date"])} · {e(variants)}</small><h3><a href="{path}index.html">{e(r["title"])}</a></h3>'
                f'<p>{e(r["question"])}</p><p>{e(r["finding"])}</p><a href="{path}report.json">JSON</a></article>'
            )
            title = r["title"].replace("[", "\\[").replace("]", "\\]")
            md += [
                f"- [{title}]({path}report.json) · {r['date'] or 'Date unreported'} · {r['finding']}"
            ]
        body += "</section>"
        md += [""]
    body += """<p id="empty" hidden>No matching reports.</p><script>
    document.querySelector('#q').addEventListener('input',event=>{
      const term=event.target.value.trim().toLowerCase();
      for(const group of document.querySelectorAll('section')){
        for(const row of group.querySelectorAll('article'))
          row.hidden=!(group.querySelector('h2').textContent+' '+row.textContent).toLowerCase().includes(term);
        group.hidden=!group.querySelector('article:not([hidden])');
      }
      document.querySelector('#empty').hidden=!!document.querySelector('section:not([hidden])');
    });</script>"""
    return page(
        "Benchmark library",
        '<span>Benchmark library</span><a href="index.json">Catalog JSON</a>',
        body,
        css,
    ), "\n".join(md)


def main(args, css):
    if args not in (["build"], ["check"]):
        raise ValueError("usage: wf bench reports build|check")
    schema = read(LIB / "schema/report-v1.schema.json")
    outputs, records = {}, []
    for directory in sorted((LIB / "reports").iterdir()):
        path = directory / "report.json"
        try:
            r = load_report(path, schema)
        except (OSError, ValueError) as err:
            raise ValueError(f"{path.relative_to(ROOT)}: {err}") from err
        records.append(r)
        outputs[directory / "index.html"] = report(r, css)
    records.sort(
        key=lambda r: (
            r["date"] or "",
            any(o["statistic"] == "raw" for o in r["observations"]),
            r["id"],
        ),
        reverse=True,
    )
    html, md = catalog(records, css)
    outputs[LIB / "index.html"], outputs[LIB / "README.md"] = html, md
    outputs[LIB / "index.json"] = encode(
        dict(
            schemaVersion=1,
            reports=[
                {
                    **{
                        k: r[k]
                        for k in (
                            "id",
                            "title",
                            "subject",
                            "date",
                            "question",
                            "finding",
                            "variants",
                        )
                    },
                    "record": f"reports/{r['id']}/report.json",
                    "page": f"reports/{r['id']}/index.html",
                }
                for r in records
            ],
        )
    )
    stale = [p for p, s in outputs.items() if not p.is_file() or p.read_text() != s]
    if args == ["check"] and stale:
        raise ValueError(
            "stale views: " + ", ".join(str(p.relative_to(ROOT)) for p in stale)
        )
    for p in stale:
        with tempfile.NamedTemporaryFile("w", dir=p.parent, delete=False) as tmp:
            tmp.write(outputs[p])
        os.chmod(tmp.name, 0o644)
        os.replace(tmp.name, p)
    return {"ok": True, "reports": len(records), "updated": len(stale)}


if __name__ == "__main__":
    try:
        print(encode(main(sys.argv[2:], sys.argv[1])).strip())
    except (OSError, ValueError, KeyError) as err:
        print(encode({"ok": False, "errors": [str(err)]}).strip())
        sys.exit(1)
