#!/usr/bin/env python3
import argparse
import json
import os
import re
import subprocess
import sys

WIRE = os.path.dirname(os.path.abspath(__file__))
RULES_FILE = os.path.join(WIRE, "rules.tsv")
JAVA_DIR = os.path.join(WIRE, "java")
SIDES = ("java", "rust")
MAX_DIFFS_PER_CASE = 20

HELP = """\
Commands:
  normalize <tree> <out>   Write the normalized form of every case in <tree>
                           to <out>/<case>/{response.json,error.txt,aws.json}.
  compare <treeA> <treeB>  Normalize both trees in memory and report every
                           difference. Exit status 1 when any case differs.

A tree holds one directory per case with the side files
<side>-response.json, <side>-error.txt and <side>-aws.json, where <side> is
java or rust. Normalization rules come from rules.tsv, applied in file order.
"""


class Rule(object):
    def __init__(self, name, scope, path, action, argument, reason):
        self.name = name
        self.scope = scope
        self.path = [p for p in path.split("/") if p != ""]
        self.action = action
        self.argument = argument
        self.reason = reason


def load_rules():
    rules = []
    with open(RULES_FILE, encoding="utf-8") as f:
        lines = f.read().splitlines()
    for number, line in enumerate(lines[1:], start=2):
        if not line.strip() or line.startswith("#"):
            continue
        cols = line.split("\t")
        if len(cols) != 6:
            raise SystemExit("rules.tsv:%d: expected 6 columns, found %d" % (number, len(cols)))
        rules.append(Rule(*cols))
    return rules


def children(node):
    if isinstance(node, dict):
        return list(node.keys())
    if isinstance(node, list):
        return list(range(len(node)))
    return []


def apply_at(node, path, fn):
    if not path:
        return fn(node)
    head, rest = path[0], path[1:]
    if head == "**":
        node = apply_at(node, rest, fn)
        for key in children(node):
            node[key] = apply_at(node[key], path, fn)
        return node
    for key in children(node):
        if head == "*" or str(key) == head:
            node[key] = apply_at(node[key], rest, fn)
    return node


def natural_key(value):
    text = json.dumps(value, sort_keys=True, separators=(",", ":"))
    return [(0, int(part), "") if part.isdigit() else (1, 0, part) for part in re.split(r"(\d+)", text)]


class ArrowDecoder(object):
    def __init__(self):
        self.cache = {}
        self.pending = []

    def request(self, schema, records):
        key = (schema, records)
        if key not in self.cache:
            self.cache[key] = None
            self.pending.append(key)
        return key

    def run(self):
        if not self.pending:
            return
        argfile = os.path.join(JAVA_DIR, "target", "argfile")
        if not os.path.exists(argfile):
            raise SystemExit("diff.py: %s is missing; run run.sh once to build the Java helper" % argfile)
        lines = [json.dumps({"schema": s, "records": r}) for s, r in self.pending]
        cmd = ["java", "--add-opens=java.base/java.nio=ALL-UNNAMED",
               "-Dlog4j2.configurationFile=parity-log4j2.xml",
               "-Dparity.log.file=" + os.devnull,
               "@" + argfile, "com.amazonaws.athena.parity.wire.ArrowDecode"]
        proc = subprocess.run(cmd, input=("\n".join(lines) + "\n").encode("utf-8"),
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if proc.returncode != 0:
            raise SystemExit("ArrowDecode failed: " + proc.stderr.decode("utf-8", "replace"))
        out = [l for l in proc.stdout.decode("utf-8").splitlines() if l.strip()]
        if len(out) != len(self.pending):
            raise SystemExit("ArrowDecode returned %d results for %d inputs" % (len(out), len(self.pending)))
        for key, line in zip(self.pending, out):
            self.cache[key] = json.loads(line)
        self.pending = []

    def get(self, key):
        return self.cache[key]


def schema_text(value):
    if isinstance(value, str):
        return value
    if isinstance(value, dict) and set(value.keys()) == {"schema"} and isinstance(value["schema"], str):
        return value["schema"]
    return None


def walk_arrow(node, decoder, resolve):
    if isinstance(node, dict):
        if "aId" in node and "schema" in node and "records" in node and schema_text(node["schema"]) is not None:
            key = decoder.request(schema_text(node["schema"]), node["records"] if isinstance(node["records"], str) else None)
            if resolve:
                decoded = decoder.get(key)
                result = {"aId": node["aId"]}
                result.update(decoded)
                for extra in node:
                    if extra not in ("aId", "schema", "records"):
                        result[extra] = walk_arrow(node[extra], decoder, resolve)
                return result
            return node
        for k in list(node.keys()):
            if k == "schema" and schema_text(node[k]) is not None:
                key = decoder.request(schema_text(node[k]), None)
                if resolve:
                    node[k] = decoder.get(key)
            else:
                node[k] = walk_arrow(node[k], decoder, resolve)
        return node
    if isinstance(node, list):
        return [walk_arrow(item, decoder, resolve) for item in node]
    return node


def walk_s3(aws, response, decoder, resolve):
    schema = None
    if isinstance(response, dict):
        schema = schema_text(response.get("schema"))
    objects = aws.get("s3Objects", [])
    for obj in objects:
        if "bytes" not in obj:
            continue
        if obj["path"].endswith(".schema"):
            key = decoder.request(obj["bytes"], None)
        elif schema is not None:
            key = decoder.request(schema, obj["bytes"])
        else:
            key = None
        if resolve and key is not None:
            obj["decoded"] = decoder.get(key)
            del obj["bytes"]
            obj.pop("size", None)
    return aws


def apply_rule(rule, doc):
    if rule.action == "replace":
        literal = json.loads(rule.argument)
        return apply_at(doc, rule.path, lambda v: v if v is None else literal)
    if rule.action == "regex":
        pattern, replacement = rule.argument.split("=>", 1)
        compiled = re.compile(pattern)
        return apply_at(doc, rule.path, lambda v: compiled.sub(replacement, v) if isinstance(v, str) else v)
    if rule.action == "sort":
        return apply_at(doc, rule.path, lambda v: sorted(v, key=natural_key) if isinstance(v, list) else v)
    if rule.action == "remove-item":
        literal = json.loads(rule.argument)
        return apply_at(doc, rule.path, lambda v: [x for x in v if x != literal] if isinstance(v, list) else v)
    raise SystemExit("rules.tsv: unknown action %s in rule %s" % (rule.action, rule.name))


class Case(object):
    def __init__(self, name, side, response, error, aws, raw_response):
        self.name = name
        self.side = side
        self.response = response
        self.error = error
        self.aws = aws
        self.raw_response = raw_response


def find_side(case_dir):
    for side in SIDES:
        for suffix in ("-response.json", "-error.txt"):
            if os.path.exists(os.path.join(case_dir, side + suffix)):
                return side
    return None


def load_tree(tree):
    cases = {}
    if not os.path.isdir(tree):
        raise SystemExit("not a directory: " + tree)
    for name in sorted(os.listdir(tree)):
        case_dir = os.path.join(tree, name)
        if not os.path.isdir(case_dir):
            continue
        side = find_side(case_dir)
        if side is None and not os.path.isfile(os.path.join(case_dir, "case.json")):
            continue
        response = error = aws = raw = None
        if side is not None:
            path = os.path.join(case_dir, side + "-response.json")
            if os.path.exists(path):
                with open(path, "rb") as f:
                    raw = f.read()
                try:
                    response = json.loads(raw.decode("utf-8"))
                except ValueError as e:
                    response = {"__invalid_json__": str(e)}
            path = os.path.join(case_dir, side + "-error.txt")
            if os.path.exists(path):
                with open(path, encoding="utf-8") as f:
                    error = f.read()
            path = os.path.join(case_dir, side + "-aws.json")
            if os.path.exists(path):
                with open(path, encoding="utf-8") as f:
                    aws = json.load(f)
        cases[name] = Case(name, side, response, error, aws, raw)
    return cases


def normalize_tree(tree, rules):
    cases = load_tree(tree)
    decoder = ArrowDecoder()
    decode_response = any(r.action == "decode-arrow" and r.scope == "response" for r in rules)
    decode_s3 = any(r.action == "decode-arrow" and r.scope == "aws" for r in rules)
    for case in cases.values():
        if decode_response and case.response is not None:
            walk_arrow(case.response, decoder, False)
        if decode_s3 and case.aws is not None:
            walk_s3(case.aws, case.response, decoder, False)
    decoder.run()
    for case in cases.values():
        if decode_s3 and case.aws is not None:
            case.aws = walk_s3(case.aws, case.response, decoder, True)
        if decode_response and case.response is not None:
            case.response = walk_arrow(case.response, decoder, True)
        for rule in rules:
            if rule.action == "decode-arrow":
                continue
            if rule.scope == "response" and case.response is not None:
                case.response = apply_rule(rule, case.response)
            elif rule.scope == "aws" and case.aws is not None:
                case.aws = apply_rule(rule, case.aws)
            elif rule.scope == "error" and case.error is not None:
                case.error = apply_rule(rule, case.error)
        if case.aws is not None and not case.aws.get("calls") and not case.aws.get("s3Objects"):
            case.aws = None
    return cases


def is_scalar(value):
    return not isinstance(value, (dict, list))


def is_flat(value):
    if isinstance(value, dict):
        return False
    if isinstance(value, list):
        return all(is_flat(v) for v in value)
    return True


def dump(value, indent=0):
    pad = "  " * indent
    inner = "  " * (indent + 1)
    if isinstance(value, dict):
        if not value:
            return "{}"
        items = ["%s%s: %s" % (inner, json.dumps(k, ensure_ascii=False), dump(value[k], indent + 1)) for k in sorted(value)]
        return "{\n" + ",\n".join(items) + "\n" + pad + "}"
    if isinstance(value, list):
        if not value:
            return "[]"
        if all(is_scalar(v) for v in value):
            return json.dumps(value, ensure_ascii=False)
        items = [inner + (json.dumps(v, ensure_ascii=False) if is_flat(v) else dump(v, indent + 1)) for v in value]
        return "[\n" + ",\n".join(items) + "\n" + pad + "]"
    return json.dumps(value, ensure_ascii=False)


def write_normalized(cases, out):
    os.makedirs(out, exist_ok=True)
    for case in cases.values():
        case_dir = os.path.join(out, case.name)
        os.makedirs(case_dir, exist_ok=True)
        for fname in ("response.json", "error.txt", "aws.json"):
            path = os.path.join(case_dir, fname)
            if os.path.exists(path):
                os.remove(path)
        if case.response is not None:
            with open(os.path.join(case_dir, "response.json"), "w", encoding="utf-8") as f:
                f.write(dump(case.response) + "\n")
        if case.error is not None:
            with open(os.path.join(case_dir, "error.txt"), "w", encoding="utf-8") as f:
                f.write(case.error)
        if case.aws is not None:
            with open(os.path.join(case_dir, "aws.json"), "w", encoding="utf-8") as f:
                f.write(dump(case.aws) + "\n")


def json_diffs(a, b, path, out):
    if len(out) >= MAX_DIFFS_PER_CASE:
        return
    if type(a) != type(b):
        out.append("%s: %s != %s" % (path or "/", short(a), short(b)))
        return
    if isinstance(a, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a:
                out.append("%s/%s: only in B: %s" % (path, k, short(b[k])))
            elif k not in b:
                out.append("%s/%s: only in A: %s" % (path, k, short(a[k])))
            else:
                json_diffs(a[k], b[k], "%s/%s" % (path, k), out)
            if len(out) >= MAX_DIFFS_PER_CASE:
                return
        return
    if isinstance(a, list):
        if len(a) != len(b):
            out.append("%s: length %d != %d" % (path or "/", len(a), len(b)))
        for i in range(min(len(a), len(b))):
            json_diffs(a[i], b[i], "%s/%d" % (path, i), out)
            if len(out) >= MAX_DIFFS_PER_CASE:
                return
        return
    if a != b:
        out.append("%s: %s != %s" % (path or "/", short(a), short(b)))


def short(value):
    text = json.dumps(value, ensure_ascii=False, sort_keys=True)
    return text if len(text) <= 160 else text[:157] + "..."


def compare(tree_a, tree_b, rules):
    a = normalize_tree(tree_a, rules)
    b = normalize_tree(tree_b, rules)
    names = sorted(set(a) | set(b))
    identical = different = only_a = only_b = 0
    by_group = {}
    for name in names:
        group = name.split("__")[0]
        stats = by_group.setdefault(group, [0, 0])
        if name not in b or b[name].side is None:
            only_a += 1
            stats[1] += 1
            print("MISSING_B\t%s" % name)
            continue
        if name not in a or a[name].side is None:
            only_b += 1
            stats[1] += 1
            print("MISSING_A\t%s" % name)
            continue
        ca, cb = a[name], b[name]
        diffs = []
        if (ca.response is None) != (cb.response is None) or (ca.error is None) != (cb.error is None):
            diffs.append("outcome: A %s, B %s" % ("error" if ca.error is not None else "response",
                                                  "error" if cb.error is not None else "response"))
        if ca.response is not None and cb.response is not None:
            json_diffs(ca.response, cb.response, "", diffs)
        if ca.error is not None and cb.error is not None and ca.error != cb.error:
            la, lb = ca.error.splitlines(), cb.error.splitlines()
            for i in range(max(len(la), len(lb))):
                x = la[i] if i < len(la) else "<none>"
                y = lb[i] if i < len(lb) else "<none>"
                if x != y:
                    diffs.append("error line %d: %s != %s" % (i + 1, x, y))
        aws_diffs = []
        json_diffs(ca.aws, cb.aws, "aws", aws_diffs)
        diffs.extend(aws_diffs)
        if diffs:
            different += 1
            stats[1] += 1
            print("DIFF\t%s" % name)
            for d in diffs:
                print("\t%s" % d)
        else:
            identical += 1
            stats[0] += 1
    for group in sorted(by_group):
        print("GROUP\t%s\tidentical=%d\tdifferent=%d" % (group, by_group[group][0], by_group[group][1]))
    print("SUMMARY\tcases=%d\tidentical=%d\tdifferent=%d\tmissing_in_a=%d\tmissing_in_b=%d"
          % (len(names), identical, different, only_b, only_a))
    return 0 if different == 0 and only_a == 0 and only_b == 0 else 1


def main():
    parser = argparse.ArgumentParser(description="Normalize and diff Lambda wire responses.", epilog=HELP,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command")
    p = sub.add_parser("normalize")
    p.add_argument("tree")
    p.add_argument("out")
    p = sub.add_parser("compare")
    p.add_argument("tree_a")
    p.add_argument("tree_b")
    sub.add_parser("rules")
    args = parser.parse_args()
    rules = load_rules()
    if args.command == "normalize":
        cases = normalize_tree(args.tree, rules)
        write_normalized(cases, args.out)
        print("normalized %d cases into %s" % (len(cases), args.out))
        return 0
    if args.command == "compare":
        return compare(args.tree_a, args.tree_b, rules)
    if args.command == "rules":
        for r in rules:
            print("%s\t%s\t/%s\t%s\t%s" % (r.name, r.scope, "/".join(r.path), r.action, r.reason))
        return 0
    parser.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
