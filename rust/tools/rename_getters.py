#!/usr/bin/env python3
import argparse
import json
import os
import re
import subprocess
import sys
from collections import defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CRATES = ["athena-federation-sdk", "athena-federation-substrait"]

KEYWORDS = {
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
    "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait",
    "true", "type", "unsafe", "use", "where", "while", "abstract", "become", "box", "do",
    "final", "macro", "override", "priv", "try", "typeof", "unsized", "virtual", "yield",
    "gen",
}

KEEP = {
    "Marker::get_value_block": "builds a new Block from the marker value",
    "get_single_value": "computes the value and fails when the set holds more than one",
    "get_span": "computes a new Range from the first and last ranges",
    "Block::get_size": "sums the buffer sizes of every vector",
    "Block::get_arrow_record_batch": "encodes the block into a new ArrowRecordBatch",
    "Block::get_record_batch": "builds a new RecordBatch from the vectors",
    "EnvironmentProperties::get_env_map": "reads the process environment",
    "SpillWriter::get_request_headers_from_env": "parses JSON from the config options",
    "QueryPassthroughSignature::get_function_signature": "formats a new String",
    "QueryPassthroughSignature::get_query_passthrough_capabilities": "builds new sub types",
    "TableName::get_qualified_table_name": "formats a new String",
    "RelDataType::get_full_type_string": "formats a new String",
    "SqlLiteral::get_value": "converts the literal into a new LiteralObject",
    "AwsServiceError::get_message": "formats the message from the SDK error",
    "CacheEntry::get_age": "reads the system clock",
    "JsonParser::get_text": "renders the current JSON token",
    "JsonParser::get_value_as_string": "renders the current JSON token",
    "JsonParser::get_value_as_int": "parses the current JSON token",
    "JsonParser::get_value_as_long": "parses the current JSON token",
    "JsonParser::get_value_as_boolean": "parses the current JSON token",
    "JsonParser::get_binary_value": "decodes the current JSON token from base64",
}

FORCE = {
    "Marker::get_value": "returns the stored value and fails on a null marker, as Java does",
}

IDENT = re.compile(r"[A-Za-z0-9_$]")
RAW_START = re.compile(r'(?:br|r)(#*)"')


def rust_files():
    out = []
    for crate in CRATES:
        base = os.path.join(ROOT, crate)
        for dirpath, dirnames, filenames in os.walk(base):
            dirnames[:] = sorted(d for d in dirnames if d != "target")
            for name in sorted(filenames):
                if name.endswith(".rs"):
                    out.append(os.path.join(dirpath, name))
    return sorted(out)


def mask(src):
    out = list(src)
    n = len(src)

    def blank(a, b):
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    i = 0
    while i < n:
        c = src[i]
        prev_ident = i > 0 and IDENT.match(src[i - 1]) is not None
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
            continue
        if src.startswith("/*", i):
            depth = 1
            j = i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            blank(i, j)
            i = j
            continue
        m = RAW_START.match(src, i)
        if m and not prev_ident:
            close = '"' + m.group(1)
            end = src.find(close, m.end())
            j = n if end < 0 else end + len(close)
            blank(i, j)
            i = j
            continue
        if c == '"' or (c == "b" and src.startswith('b"', i) and not prev_ident):
            j = i + (2 if c == "b" else 1)
            while j < n:
                if src[j] == "\\":
                    j += 2
                elif src[j] == '"':
                    j += 1
                    break
                else:
                    j += 1
            blank(i, j)
            i = j
            continue
        if c == "'":
            if i + 1 < n and src[i + 1] == "\\":
                j = i + 3
                while j < n and src[j] != "'":
                    j += 1
                blank(i, j + 1)
                i = j + 1
                continue
            if i + 2 < n and src[i + 2] == "'":
                blank(i, i + 3)
                i += 3
                continue
        i += 1
    return "".join(out)


def brace_map(masked):
    pairs = {}
    stack = []
    for idx, ch in enumerate(masked):
        if ch == "{":
            stack.append(idx)
        elif ch == "}" and stack:
            pairs[stack.pop()] = idx
    return pairs


def match_close(text, start, open_ch, close_ch):
    depth = 0
    for k in range(start, len(text)):
        if text[k] == open_ch:
            depth += 1
        elif text[k] == close_ch:
            depth -= 1
            if depth == 0:
                return k
    return -1


def split_top(text, sep):
    parts = []
    depth = 0
    cur = []
    for ch in text:
        if ch in "<([":
            depth += 1
        elif ch in ">)]":
            depth -= 1
        if ch == sep and depth == 0:
            parts.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    parts.append("".join(cur))
    return [p.strip() for p in parts if p.strip()]


def last_segment(path):
    path = path.strip()
    path = re.sub(r"^(?:dyn|&|mut|\s)+", "", path)
    depth = 0
    cut = len(path)
    for k, ch in enumerate(path):
        if ch == "<":
            if depth == 0:
                cut = k
                break
            depth += 1
    path = path[:cut]
    seg = path.split("::")[-1].strip()
    return seg


ITEM_RE = re.compile(r"(?:(?<=[\s;{}\]])|^)(?:unsafe\s+)?(impl|trait)\b")
FN_RE = re.compile(r"\b(?:(async)\s+)?fn\s+(get_\w+)\b")
ANY_FN_RE = re.compile(r"\bfn\s+([A-Za-z_]\w*)\b")


class Block(object):
    def __init__(self, kind, trait, owner, open_idx, close_idx):
        self.kind = kind
        self.trait = trait
        self.owner = owner
        self.open = open_idx
        self.close = close_idx


def find_blocks(masked, pairs):
    blocks = []
    for m in ITEM_RE.finditer(masked):
        brace = masked.find("{", m.end())
        semi = masked.find(";", m.end())
        if brace < 0 or (0 <= semi < brace) or brace not in pairs:
            continue
        header = masked[m.end():brace].strip()
        if m.group(1) == "trait":
            name = re.match(r"(\w+)", header)
            if not name:
                continue
            blocks.append(Block("trait", name.group(1), name.group(1), brace, pairs[brace]))
            continue
        if header.startswith("<"):
            end = match_close(header, 0, "<", ">")
            header = header[end + 1:]
        header = re.split(r"\bwhere\b", header)[0]
        parts = re.split(r"\s+for\s+", header, maxsplit=1)
        if len(parts) == 2:
            blocks.append(Block("trait_impl", last_segment(parts[0]), last_segment(parts[1]),
                                brace, pairs[brace]))
        else:
            blocks.append(Block("inherent", None, last_segment(parts[0]), brace, pairs[brace]))
    return blocks


def enclosing(blocks, pos):
    best = None
    for b in blocks:
        if b.open < pos < b.close and (best is None or b.open > best.open):
            best = b
    return best


def parse_fn(masked, pairs, name_end):
    k = name_end
    while k < len(masked) and masked[k].isspace():
        k += 1
    if k < len(masked) and masked[k] == "<":
        k = match_close(masked, k, "<", ">") + 1
        while masked[k].isspace():
            k += 1
    if masked[k] != "(":
        return None
    close = match_close(masked, k, "(", ")")
    params = split_top(masked[k + 1:close], ",")
    j = close + 1
    while j < len(masked) and masked[j] not in "{;":
        j += 1
    ret = masked[close + 1:j].strip()
    ret = re.sub(r"^->\s*", "", ret)
    body = None
    if masked[j] == "{":
        body = masked[j + 1:pairs[j]]
    return params, ret, body


SELF_PARAM = re.compile(r"^(?:&\s*(?:'\w+\s+)?(?:mut\s+)?|mut\s+)?self(?:\s*:.*)?$")


def is_simple(body):
    if body is None:
        return True
    inner = body.strip()
    return re.search(r"\blet\b|;|\bfor\b|\bwhile\b|\bloop\b|\breturn\b", inner) is None


class Def(object):
    def __init__(self, path, pos, name, is_async, params, ret, body, block):
        self.path = path
        self.pos = pos
        self.name = name
        self.is_async = is_async
        self.params = params
        self.ret = ret
        self.body = body
        self.block = block

    @property
    def zero_arg(self):
        return (len(self.params) == 1 and SELF_PARAM.match(self.params[0]) is not None)

    @property
    def owner(self):
        return self.block.owner if self.block else None

    def where(self):
        rel = os.path.relpath(self.path, ROOT)
        return rel


def analyse(files):
    sources = {}
    masks = {}
    defs = []
    methods_by_owner = defaultdict(set)
    for path in files:
        with open(path, encoding="utf-8") as fh:
            src = fh.read()
        masked = mask(src)
        sources[path] = src
        masks[path] = masked
        pairs = brace_map(masked)
        blocks = find_blocks(masked, pairs)
        for m in ANY_FN_RE.finditer(masked):
            block = enclosing(blocks, m.start())
            if block:
                methods_by_owner[block.owner].add(m.group(1))
        for m in FN_RE.finditer(masked):
            block = enclosing(blocks, m.start())
            if block is None:
                continue
            parsed = parse_fn(masked, pairs, m.end())
            if parsed is None:
                continue
            params, ret, body = parsed
            defs.append(Def(path, m.start(2), m.group(2), bool(m.group(1)), params, ret, body,
                            block))
    return sources, masks, defs, methods_by_owner


def keep_reason(d, methods_by_owner):
    new = d.name[len("get_"):]
    if d.is_async:
        return "async I/O call, not a getter"
    if new in KEYWORDS:
        return "dropping the prefix gives the keyword `{}`".format(new)
    for key in ("{}::{}".format(d.owner, d.name), d.name):
        if key in KEEP:
            return KEEP[key]
    forced = "{}::{}".format(d.owner, d.name) in FORCE
    if d.block.kind != "trait_impl" and not forced and not is_simple(d.body):
        return "computes its result"
    if new in methods_by_owner[d.owner]:
        return "`{}` already exists on {}".format(new, d.owner)
    return None


def decide(defs, methods_by_owner):
    zero = [d for d in defs if d.zero_arg]
    groups = defaultdict(list)
    for d in zero:
        if d.block.kind == "inherent":
            key = ("inherent", d.owner, d.path, d.pos)
        else:
            key = ("trait", d.block.trait, d.name)
        groups[key].append(d)
    decisions = {}
    for key in sorted(groups, key=str):
        members = groups[key]
        reasons = [keep_reason(d, methods_by_owner) for d in members]
        reason = next((r for r in reasons if r), None)
        if key[0] == "trait" and not any(d.block.kind == "trait" for d in members):
            reason = reason or "implements a trait method defined outside this workspace"
        for d in members:
            decisions[(d.path, d.pos)] = reason

    renamed = {}
    kept = defaultdict(list)
    for d in zero:
        reason = decisions[(d.path, d.pos)]
        label = "{}::{}".format(d.owner, d.name)
        if reason:
            kept[d.name].append((label, reason))
        else:
            info = renamed.setdefault(d.name, {"new": d.name[len("get_"):], "defs": [],
                                               "async_twin": False})
            info["defs"].append(d)
    for name in sorted(renamed):
        info = renamed[name]
        info["async_twin"] = any(d.is_async for d in zero if d.name == name)
        info["kept_twin"] = bool(kept.get(name))
    return renamed, kept


def rewrite_text(src, masked, renamed, def_positions):
    edits = []
    for pos in def_positions:
        name = re.match(r"get_\w+", masked[pos:]).group(0)
        edits.append((pos, pos + len(name), renamed[name]["new"]))
    names = "|".join(sorted((n for n in renamed if not renamed[n]["kept_twin"]), key=len,
                            reverse=True))
    if names:
        call = re.compile(r"\.\s*(" + names + r")\b(?=\s*\(\s*\))")
        for m in call.finditer(masked):
            name = m.group(1)
            if renamed[name]["async_twin"]:
                tail = masked[m.end():m.end() + 64]
                if re.match(r"\s*\(\s*\)\s*\.\s*await\b", tail):
                    continue
            edits.append((m.start(1), m.end(1), renamed[name]["new"]))
        path_ref = re.compile(r"(?:\b[A-Z]\w*|\bSelf|>)\s*::\s*(" + names + r")\b")
        for m in path_ref.finditer(masked):
            edits.append((m.start(1), m.end(1), renamed[m.group(1)]["new"]))
    uniq = {}
    for e in edits:
        uniq[e[0]] = e
    edits = sorted(uniq.values())
    out = []
    last = 0
    for start, end, new in edits:
        out.append(src[last:start])
        out.append(new)
        last = end
    out.append(src[last:])
    return "".join(out), edits


def line_col(text, offset):
    line = text.count("\n", 0, offset) + 1
    col = offset - (text.rfind("\n", 0, offset) + 1) + 1
    return line, col


def cargo_errors():
    proc = subprocess.run(
        ["cargo", "check", "--workspace", "--all-targets", "--message-format=json"],
        cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, universal_newlines=True)
    errors = []
    for line in proc.stdout.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") != "compiler-message":
            continue
        inner = msg["message"]
        if inner.get("level") != "error":
            continue
        for span in inner.get("spans", []):
            if span.get("is_primary"):
                errors.append((os.path.join(ROOT, span["file_name"]), span["line_start"],
                               span["column_start"], span["line_end"], span["column_end"],
                               inner.get("message", "")))
    return errors


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--no-check", action="store_true")
    args = parser.parse_args()

    files = rust_files()
    sources, masks, defs, methods_by_owner = analyse(files)
    renamed, kept = decide(defs, methods_by_owner)

    if args.list:
        for name in sorted(renamed):
            owners = sorted(set(d.owner for d in renamed[name]["defs"]))
            print("RENAME {} -> {} [{}]".format(name, renamed[name]["new"], ", ".join(owners)))
        for name in sorted(kept):
            for label, reason in sorted(set(kept[name])):
                print("KEEP   {}: {}".format(label, reason))
        print("renamed={} kept={}".format(len(renamed), len(kept)))
        return 0

    def_positions = defaultdict(list)
    for name, info in renamed.items():
        for d in info["defs"]:
            def_positions[d.path].append(d.pos)

    sites = {}
    for path in files:
        new_src, edits = rewrite_text(sources[path], masks[path], renamed, def_positions[path])
        if new_src != sources[path]:
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(new_src)
        shift = 0
        for start, end, new in edits:
            new_start = start + shift
            shift += len(new) - (end - start)
            line, col = line_col(new_src, new_start)
            sites[(path, line, col)] = (new_start, new, "get_" + new)

    twins = {"get_" + info["new"]: info["new"] for info in renamed.values()
             if info["kept_twin"]}
    rounds = 0
    while twins and not args.no_check:
        rounds += 1
        fixed = 0
        errors = sorted(set(cargo_errors()), key=lambda e: (e[0], e[1], -e[2]))
        for path, l1, c1, l2, c2, message in errors:
            m = re.match(r"no (?:method|function or associated item) named `(\w+)` found",
                         message)
            if not m or m.group(1) not in twins or l1 != l2:
                continue
            old = m.group(1)
            with open(path, encoding="utf-8") as fh:
                lines = fh.read().split("\n")
            line = lines[l1 - 1]
            if line[c1 - 1:c1 - 1 + len(old)] != old:
                continue
            lines[l1 - 1] = line[:c1 - 1] + twins[old] + line[c1 - 1 + len(old):]
            with open(path, "w", encoding="utf-8") as fh:
                fh.write("\n".join(lines))
            fixed += 1
        if not fixed or rounds > 10:
            break

    reverted = []
    rounds = 0
    while not args.no_check:
        rounds += 1
        hits = {}
        for path, l1, c1, l2, c2, message in cargo_errors():
            for (spath, line, col), site in sites.items():
                if spath == path and l1 == line and c1 <= col < (c2 if l2 == l1 else 10 ** 6):
                    hits[(spath, line, col)] = message
        if not hits or rounds > 10:
            break
        for key in sorted(hits, key=lambda k: (k[0], -k[1], -k[2])):
            path = key[0]
            offset, new, old = sites.pop(key)
            with open(path, encoding="utf-8") as fh:
                text = fh.read()
            text = text[:offset] + old + text[offset + len(new):]
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(text)
            for other in list(sites):
                if other[0] == path and other[1] == key[1] and other[2] > key[2]:
                    o_off, o_new, o_old = sites.pop(other)
                    delta = len(old) - len(new)
                    sites[(path, other[1], other[2] + delta)] = (o_off + delta, o_new, o_old)
                elif other[0] == path and sites[other][0] > offset:
                    o_off, o_new, o_old = sites[other]
                    sites[other] = (o_off + len(old) - len(new), o_new, o_old)
            reverted.append("{}:{}:{} {} ({})".format(os.path.relpath(path, ROOT), key[1],
                                                       key[2], old, hits[key]))

    subprocess.run(["cargo", "fmt", "--all"], cwd=ROOT, check=True)
    print("renamed {} getter names at {} sites".format(len(renamed), len(sites)))
    for line in reverted:
        print("reverted " + line)
    return 0


if __name__ == "__main__":
    sys.exit(main())
