#!/usr/bin/env python3
import argparse
import os
import re
import sys

RUST_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO_ROOT = os.path.dirname(RUST_ROOT)
DEFAULT_TABLE = os.path.join(RUST_ROOT, "parity", "classes.tsv")
MAIN_ROOT = os.path.join("src", "main", "java")
HEADER = ["java_class", "rust_path", "status", "note"]
VALID_STATUS = ("ported", "merged", "not_ported")
TYPE_KEYWORDS = ("class", "interface", "enum", "record")
RUST_ITEM = r"\b(?:struct|enum|trait|type|fn|mod|const|static|macro_rules!)\s+{}\b"

HELP_EPILOG = """\
classes.tsv has the header java_class<TAB>rust_path<TAB>status<TAB>note.
java_class is the fully qualified name of a public top-level Java type.
rust_path is <file relative to rust/>::<item>, for example
athena-federation-sdk/src/domain/split.rs::Split. The file must exist and
define the item as a struct, enum, trait, type, fn, mod, const or static.
status is ported (the item is the Rust counterpart), merged (the Java type
folded into the named item) or not_ported (rust_path is - and the note gives
the reason). ported and merged rows also need a rust_path.

Output:
  FINDING <kind> <line or -> <java_class> <detail>
  SUMMARY <module> java_classes=N ported=N merged=N not_ported=N
  RESULT PASS|FAIL findings=N
  Finding kinds: missing_row, unknown_java_class, duplicate_row, bad_status,
  missing_rust_file, missing_rust_item, bad_rust_path, not_ported_without_note,
  malformed_row.
"""


def strip_java(text):
    out = []
    i = 0
    n = len(text)
    while i < n:
        if text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2)
            i = n if j < 0 else j + 2
            out.append(" ")
        elif text.startswith('"""', i):
            j = text.find('"""', i + 3)
            i = n if j < 0 else j + 3
            out.append('""')
        elif text[i] in "\"'":
            q = text[i]
            j = i + 1
            while j < n and text[j] != q and text[j] != "\n":
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            out.append('""')
        else:
            out.append(text[i])
            i += 1
    return "".join(out)


def public_top_level_types(text):
    code = strip_java(text)
    package = ""
    m = re.search(r"\bpackage\s+([\w.]+)\s*;", code)
    if m:
        package = m.group(1)
    found = []
    depth = 0
    statement = []
    for tok in re.findall(r"@interface|[\w$]+|[{};]|\S", code):
        if tok == "{":
            if depth == 0:
                name = declared_type(statement)
                if name:
                    found.append(name)
            depth += 1
            statement = []
        elif tok == "}":
            depth -= 1
            statement = []
        elif tok == ";":
            if depth == 0:
                statement = []
        elif depth == 0:
            statement.append(tok)
    return [package + "." + name if package else name for name in found]


def declared_type(tokens):
    for k, tok in enumerate(tokens):
        if tok in TYPE_KEYWORDS or tok == "@interface":
            if k > 0 and tokens[k - 1] == ".":
                continue
            if "public" not in tokens[:k]:
                return None
            if k + 1 < len(tokens):
                return tokens[k + 1]
            return None
    return None


def java_classes(module):
    root = os.path.join(REPO_ROOT, module, MAIN_ROOT)
    if not os.path.isdir(root):
        raise SystemExit("no Java sources at " + root)
    result = {}
    for dirpath, _, files in os.walk(root):
        for f in sorted(files):
            if not f.endswith(".java"):
                continue
            path = os.path.join(dirpath, f)
            with open(path, encoding="utf-8") as fh:
                for fq in public_top_level_types(fh.read()):
                    result[fq] = os.path.relpath(path, REPO_ROOT)
    return result


def java_class_exists(fq):
    rel = fq.replace(".", os.sep) + ".java"
    for entry in os.listdir(REPO_ROOT):
        if os.path.isfile(os.path.join(REPO_ROOT, entry, MAIN_ROOT, rel)):
            return True
    return False


def read_table(path):
    rows = []
    with open(path, encoding="utf-8") as fh:
        lines = fh.read().split("\n")
    if not lines or lines[0].split("\t") != HEADER:
        raise SystemExit(path + ": first line must be " + "\\t".join(HEADER))
    for number, line in enumerate(lines[1:], start=2):
        if line.strip():
            rows.append((number, line.split("\t")))
    return rows


def check_rust_path(rust_path, cache):
    if "::" not in rust_path:
        return "bad_rust_path", "expected <file>::<item>"
    file_part, item = rust_path.split("::", 1)
    full = os.path.join(RUST_ROOT, file_part)
    if not os.path.isfile(full):
        return "missing_rust_file", file_part
    if full not in cache:
        with open(full, encoding="utf-8") as fh:
            cache[full] = fh.read()
    name = item.split("::")[-1]
    if not re.search(RUST_ITEM.format(re.escape(name)), cache[full]):
        return "missing_rust_item", rust_path
    return None


def main():
    parser = argparse.ArgumentParser(
        description="Check that every public top-level Java type in a module has a row in classes.tsv.",
        epilog=HELP_EPILOG,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--module", required=True, help="Java module directory at the repo root, for example athena-jdbc")
    parser.add_argument("--classes", default=DEFAULT_TABLE, help="classes table, default rust/parity/classes.tsv")
    args = parser.parse_args()

    classes = java_classes(args.module)
    rows = read_table(args.classes)
    findings = []
    seen = {}
    counts = {status: 0 for status in VALID_STATUS}
    cache = {}

    for number, cols in rows:
        if len(cols) != len(HEADER):
            findings.append(("malformed_row", number, cols[0] if cols else "-", "expected 4 columns"))
            continue
        java_class, rust_path, status, note = cols
        in_module = java_class in classes
        if not in_module:
            if not java_class_exists(java_class):
                findings.append(("unknown_java_class", number, java_class, "no Java source for this class"))
            continue
        if java_class in seen:
            findings.append(("duplicate_row", number, java_class, "first row at line " + str(seen[java_class])))
            continue
        seen[java_class] = number
        if status not in VALID_STATUS:
            findings.append(("bad_status", number, java_class, status))
            continue
        counts[status] += 1
        if status == "not_ported":
            if not note.strip():
                findings.append(("not_ported_without_note", number, java_class, "give the reason"))
            continue
        problem = check_rust_path(rust_path, cache)
        if problem:
            findings.append((problem[0], number, java_class, problem[1]))

    for fq in sorted(classes):
        if fq not in seen:
            findings.append(("missing_row", "-", fq, classes[fq]))

    for kind, line, subject, detail in findings:
        print("\t".join(["FINDING", kind, str(line), subject, detail]))
    print(
        "\t".join(
            [
                "SUMMARY",
                args.module,
                "java_classes=" + str(len(classes)),
                "ported=" + str(counts["ported"]),
                "merged=" + str(counts["merged"]),
                "not_ported=" + str(counts["not_ported"]),
            ]
        )
    )
    print("RESULT " + ("PASS" if not findings else "FAIL") + " findings=" + str(len(findings)))
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
