#!/usr/bin/env python3
import argparse
import json
import os
import re
import subprocess
import sys
from collections import OrderedDict, defaultdict

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RUST_DIR = os.path.join(REPO, "rust")
LEDGER = os.path.join(RUST_DIR, "parity", "tests.tsv")
TEST_ROOT = os.path.join("src", "test", "java")
VALID_STATUS = ("ported", "adapted")
TYPE_KEYWORDS = ("class", "interface", "enum", "record")
JUNIT_ANNOTATIONS = {
    "Test": ("org.junit.Test", "org.junit.jupiter.api.Test", "org.testng.annotations.Test"),
    "ParameterizedTest": ("org.junit.jupiter.params.ParameterizedTest",),
    "RepeatedTest": ("org.junit.jupiter.api.RepeatedTest",),
    "TestFactory": ("org.junit.jupiter.api.TestFactory",),
}
JQWIK_ANNOTATIONS = {
    "Property": ("net.jqwik.api.Property",),
    "Example": ("net.jqwik.api.Example",),
}

HELP_EPILOG = """\
Java test ids:
  A test is fq.Class#method, for example
  com.amazonaws.athena.connector.lambda.data.BlockTest#constrainedBlockTest.
  A nested class uses dots: pkg.Outer.Inner#method.
  An inherited test is listed once per concrete subclass that runs it, as
  fq.Subclass#method, where fq.Subclass is the concrete class and method is
  declared in an abstract base class, a concrete base class or an interface.
  An abstract class has no entries of its own. A concrete base class keeps its
  own entries as well. A subclass method with the same name and no test
  annotation hides the inherited test, as JUnit does.

Rust test ids:
  rust_test is <test binary>::<test path>. The binary is the integration test
  target name (tests/foo_test.rs gives foo_test) or the crate library name for
  unit tests in src/ (athena_federation_sdk). Example:
  arrow_type_comparator_test::test_compare_big_int.

Output:
  Tab-separated lines on stdout.
  FINDING <kind> <module> <ledger line or -> <subject> <detail>
  SUMMARY <module> java_tests=N ledger_rows=N passing=N
  RESULT PASS|FAIL findings=N
  Finding kinds: missing_ledger_row, unknown_java_test, duplicate_java_test,
  duplicate_rust_test, missing_rust_test, rust_test_failed, rust_test_ignored,
  bad_status, adapted_without_note, malformed_row, cargo_error.

Exit status: 0 when there are no findings, 1 when there are findings, 2 on
usage errors.
"""


def tokenize(text):
    tokens = []
    i = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c.isspace():
            i += 1
        elif text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j + 1
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2)
            i = n if j < 0 else j + 2
        elif text.startswith('"""', i):
            j = i + 3
            while j < n and not text.startswith('"""', j):
                j += 2 if text[j] == "\\" else 1
            tokens.append('""')
            i = j + 3
        elif c == '"' or c == "'":
            j = i + 1
            while j < n and text[j] != c and text[j] != "\n":
                j += 2 if text[j] == "\\" else 1
            tokens.append('""')
            i = j + 1
        elif c.isalpha() or c == "_" or c == "$":
            j = i + 1
            while j < n and (text[j].isalnum() or text[j] in "_$"):
                j += 1
            tokens.append(text[i:j])
            i = j
        elif c.isdigit():
            j = i + 1
            while j < n and (text[j].isalnum() or text[j] in "._"):
                j += 1
            tokens.append("0")
            i = j
        else:
            tokens.append(c)
            i += 1
    return tokens


def is_ident(tok):
    return bool(tok) and (tok[0].isalpha() or tok[0] in "_$")


class JavaType(object):
    def __init__(self, fq, simple, kind, abstract, parents, path, package, imports, outer):
        self.fq = fq
        self.simple = simple
        self.kind = kind
        self.abstract = abstract
        self.parents = parents
        self.path = path
        self.package = package
        self.imports = imports
        self.outer = outer
        self.methods = OrderedDict()
        self.nested = []


class FileParser(object):
    def __init__(self, tokens, path):
        self.t = tokens
        self.path = path
        self.package = ""
        self.imports = []
        self.types = []

    def skip_balanced(self, i, open_tok, close_tok):
        depth = 0
        n = len(self.t)
        while i < n:
            if self.t[i] == open_tok:
                depth += 1
            elif self.t[i] == close_tok:
                depth -= 1
                if depth == 0:
                    return i + 1
            i += 1
        return n

    def read_qualified(self, i):
        parts = [self.t[i]]
        i += 1
        while i + 1 < len(self.t) and self.t[i] == "." and is_ident(self.t[i + 1]):
            parts.append(self.t[i + 1])
            i += 2
        return ".".join(parts), i

    def parse(self):
        i = 0
        n = len(self.t)
        while i < n:
            tok = self.t[i]
            if tok == "package":
                name, i = self.read_qualified(i + 1)
                self.package = name
            elif tok == "import":
                j = i + 1
                static = False
                if j < n and self.t[j] == "static":
                    static = True
                    j += 1
                name, j = self.read_qualified(j)
                if j + 1 < n and self.t[j] == "." and self.t[j + 1] == "*":
                    name += ".*"
                    j += 2
                if not static:
                    self.imports.append(name)
                i = j
            elif tok == ";":
                i += 1
            else:
                i = self.parse_member(i, None)
        return self

    def read_header(self, i):
        annotations = []
        header = []
        n = len(self.t)
        while i < n:
            tok = self.t[i]
            if tok == "@" and i + 1 < n and self.t[i + 1] == "interface":
                header.append("@interface")
                i += 2
                continue
            if tok == "@" and i + 1 < n and is_ident(self.t[i + 1]):
                name, i = self.read_qualified(i + 1)
                annotations.append(name)
                if i < n and self.t[i] == "(":
                    i = self.skip_balanced(i, "(", ")")
                continue
            if tok in ("{", ";", "}", "="):
                return annotations, header, i
            if tok == "(":
                header.append("(")
                i = self.skip_balanced(i, "(", ")")
                continue
            header.append(tok)
            i += 1
        return annotations, header, i

    def skip_initializer(self, i):
        n = len(self.t)
        while i < n:
            tok = self.t[i]
            if tok == ";":
                return i + 1
            if tok == "(":
                i = self.skip_balanced(i, "(", ")")
            elif tok == "{":
                i = self.skip_balanced(i, "{", "}")
            elif tok == "[":
                i = self.skip_balanced(i, "[", "]")
            elif tok == "}":
                return i
            else:
                i += 1
        return n

    def parse_member(self, i, owner):
        annotations, header, i = self.read_header(i)
        n = len(self.t)
        if i >= n:
            return n
        stop = self.t[i]
        kw_index = None
        for k, tok in enumerate(header):
            if tok in TYPE_KEYWORDS or tok == "@interface":
                if k > 0 and header[k - 1] == ".":
                    continue
                kw_index = k
                break
        if stop == "}":
            return i
        if stop == "=":
            return self.skip_initializer(i)
        if stop == ";":
            if owner is not None and "(" in header:
                self.record_method(owner, header, annotations)
            return i + 1
        if kw_index is not None:
            return self.parse_type(header, kw_index, i, owner)
        if owner is not None and "(" in header:
            self.record_method(owner, header, annotations)
        return self.skip_balanced(i, "{", "}")

    def record_method(self, owner, header, annotations):
        p = header.index("(")
        if p == 0 or not is_ident(header[p - 1]):
            return
        name = header[p - 1]
        if name == owner.simple:
            return
        anns = owner.methods.setdefault(name, [])
        anns.extend(annotations)

    def parse_type(self, header, kw_index, brace_i, owner):
        kind = header[kw_index]
        if kind == "@interface":
            return self.skip_balanced(brace_i, "{", "}")
        modifiers = header[:kw_index]
        simple = header[kw_index + 1] if kw_index + 1 < len(header) else "?"
        parents = []
        mode = None
        depth = 0
        k = kw_index + 2
        while k < len(header):
            tok = header[k]
            if tok == "<":
                depth += 1
            elif tok == ">":
                depth -= 1
            elif depth == 0 and tok in ("extends", "implements"):
                mode = "parent"
            elif depth == 0 and tok == "permits":
                mode = None
            elif depth == 0 and mode == "parent" and is_ident(tok) and (k == 0 or header[k - 1] != "."):
                parts = [tok]
                m = k + 1
                while m + 1 < len(header) and header[m] == "." and is_ident(header[m + 1]):
                    parts.append(header[m + 1])
                    m += 2
                parents.append(".".join(parts))
                k = m
                continue
            k += 1
        if owner is None:
            fq = self.package + "." + simple if self.package else simple
        else:
            fq = owner.fq + "." + simple
        abstract = "abstract" in modifiers or kind == "interface"
        jt = JavaType(fq, simple, kind, abstract, parents, self.path, self.package, self.imports, owner)
        self.types.append(jt)
        if owner is not None:
            owner.nested.append(jt)
        i = brace_i + 1
        n = len(self.t)
        if kind == "enum":
            i = self.skip_enum_constants(i)
        while i < n and self.t[i] != "}":
            if self.t[i] == ";":
                i += 1
                continue
            i = self.parse_member(i, jt)
        return i + 1

    def skip_enum_constants(self, i):
        n = len(self.t)
        while i < n:
            tok = self.t[i]
            if tok == ";":
                return i + 1
            if tok == "}":
                return i
            if tok == "(":
                i = self.skip_balanced(i, "(", ")")
            elif tok == "{":
                i = self.skip_balanced(i, "{", "}")
            else:
                i += 1
        return n


def is_test_annotation(name, imports):
    simple = name.rsplit(".", 1)[-1]
    if simple in JUNIT_ANNOTATIONS:
        return "." not in name or name in JUNIT_ANNOTATIONS[simple]
    if simple in JQWIK_ANNOTATIONS:
        if "." in name:
            return name in JQWIK_ANNOTATIONS[simple]
        for imp in imports:
            if imp in JQWIK_ANNOTATIONS[simple] or imp == "net.jqwik.api.*":
                return True
        return False
    return False


class JavaIndex(object):
    def __init__(self):
        self.types = {}
        self.by_simple = defaultdict(list)
        self.module_of = {}

    def add_module(self, module):
        root = os.path.join(REPO, module, TEST_ROOT)
        if not os.path.isdir(root):
            return False
        for dirpath, _, files in os.walk(root):
            for f in sorted(files):
                if not f.endswith(".java"):
                    continue
                full = os.path.join(dirpath, f)
                rel = os.path.relpath(full, root).replace(os.sep, "/")
                with open(full, encoding="utf-8", errors="replace") as fh:
                    parser = FileParser(tokenize(fh.read()), rel).parse()
                for jt in parser.types:
                    self.types[jt.fq] = jt
                    self.by_simple[jt.simple].append(jt.fq)
                    self.module_of[jt.fq] = module
        return True

    def resolve(self, name, jt):
        if name in self.types:
            return name
        head, _, rest = name.partition(".")
        scope = jt
        while scope is not None:
            for nested in scope.nested:
                if nested.simple == head:
                    cand = nested.fq + ("." + rest if rest else "")
                    if cand in self.types:
                        return cand
            scope = scope.outer
        for imp in jt.imports:
            if imp.endswith("." + head):
                cand = imp + ("." + rest if rest else "")
                if cand in self.types:
                    return cand
        cand = (jt.package + "." if jt.package else "") + name
        if cand in self.types:
            return cand
        for imp in jt.imports:
            if imp.endswith(".*"):
                cand = imp[:-1] + name
                if cand in self.types:
                    return cand
        if not rest and len(self.by_simple.get(name, [])) == 1:
            return self.by_simple[name][0]
        return None

    def tests_for(self, jt):
        seen = set()
        tests = []
        visited = set()
        stack = [jt]
        while stack:
            cur = stack.pop(0)
            if cur.fq in visited:
                continue
            visited.add(cur.fq)
            for name, anns in cur.methods.items():
                if name in seen:
                    continue
                seen.add(name)
                if any(is_test_annotation(a, cur.imports) for a in anns):
                    tests.append(name)
            for p in cur.parents:
                fq = self.resolve(p, cur)
                if fq is not None:
                    stack.append(self.types[fq])
        return tests

    def collect(self, module):
        result = OrderedDict()
        for fq in sorted(self.types):
            jt = self.types[fq]
            if self.module_of[fq] != module or jt.abstract or jt.kind not in ("class", "record"):
                continue
            for m in sorted(self.tests_for(jt)):
                result[fq + "#" + m] = jt.path
        return result


def glob_to_regex(pattern):
    out = []
    i = 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
        elif pattern.startswith("**", i):
            out.append(".*")
            i += 2
        elif pattern[i] == "*":
            out.append("[^/]*")
            i += 1
        elif pattern[i] == "?":
            out.append("[^/]")
            i += 1
        else:
            out.append(re.escape(pattern[i]))
            i += 1
    return re.compile("^" + "".join(out) + "$")


def path_matches(path, patterns):
    return any(p.match(path) for p in patterns)


def fq_to_path(fq_class):
    parts = fq_class.split(".")
    for k, part in enumerate(parts):
        if part[:1].isupper():
            return "/".join(parts[: k + 1]) + ".java"
    return "/".join(parts) + ".java"


def read_ledger():
    rows = []
    with open(LEDGER, encoding="utf-8") as fh:
        lines = fh.read().splitlines()
    for lineno, line in enumerate(lines, start=1):
        if lineno == 1 or not line.strip():
            continue
        cols = line.split("\t")
        rows.append((lineno, cols))
    return rows


TEST_LINE = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)\b")
RUNNING_LINE = re.compile(r"^\s*Running (?:unittests )?\S+ \((.+)\)\s*$")
DOCTEST_LINE = re.compile(r"^\s*Doc-tests \S+")


def binary_prefix(path):
    base = os.path.basename(path.strip())
    if base.endswith(".exe"):
        base = base[:-4]
    return re.sub(r"-[0-9a-f]{8,}$", "", base)


def run_cargo(json_format):
    cmd = ["cargo", "test", "--workspace", "--no-fail-fast"]
    if json_format:
        cmd += ["--", "-Z", "unstable-options", "--format", "json"]
    try:
        proc = subprocess.run(cmd, cwd=RUST_DIR, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    except OSError as e:
        return None, None, str(e)
    return proc.returncode, proc.stdout.decode("utf-8", errors="replace"), None


def parse_cargo_output(output, json_format):
    results = {}
    errors = []
    prefix = None
    for line in output.splitlines():
        m = RUNNING_LINE.match(line)
        if m:
            prefix = binary_prefix(m.group(1))
            continue
        if DOCTEST_LINE.match(line):
            prefix = None
            continue
        if line.startswith("error") and "warning" not in line:
            errors.append(line.strip())
        if prefix is None:
            continue
        if json_format:
            if not line.startswith("{"):
                continue
            try:
                ev = json.loads(line)
            except ValueError:
                continue
            if ev.get("type") != "test" or ev.get("event") not in ("ok", "failed", "ignored"):
                continue
            status = {"ok": "ok", "failed": "FAILED", "ignored": "ignored"}[ev["event"]]
            results[prefix + "::" + ev["name"]] = status
        else:
            m = TEST_LINE.match(line)
            if m:
                results[prefix + "::" + m.group(1)] = m.group(2)
    return results, errors


def cargo_results():
    code, out, err = run_cargo(True)
    if err is not None:
        return None, ["cannot run cargo: " + err], None
    mode = "json"
    results, errors = parse_cargo_output(out, True)
    if "only accepted on the nightly" in out or (not results and code != 0):
        mode = "plain"
        code, out, err = run_cargo(False)
        if err is not None:
            return None, ["cannot run cargo: " + err], None
        results, errors = parse_cargo_output(out, False)
    build_errors = [e for e in errors if e.startswith("error[") or "could not compile" in e]
    if code != 0 and not results:
        build_errors = build_errors or ["cargo test exited " + str(code) + " with no test results"]
    return results, build_errors, mode


def main():
    parser = argparse.ArgumentParser(
        description="Check rust/parity/tests.tsv against the Java tests and the cargo test results.",
        epilog=HELP_EPILOG,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--modules", required=True, help="comma-separated Java module directories, for example athena-federation-sdk,athena-jdbc")
    parser.add_argument("--scope", nargs="+", action="append", default=[], metavar="GLOB",
                        help="limit Java tests to source paths under src/test/java, for example 'com/amazonaws/athena/connector/lambda/data/**'")
    parser.add_argument("--exclude", nargs="+", action="append", default=[], metavar="GLOB",
                        help="drop Java tests whose source path matches, for example '**/S3BlockSpillerTest.java'")
    parser.add_argument("--skip-cargo", action="store_true", help="do not run cargo; skip the Rust existence and result checks")
    args = parser.parse_args()

    modules = [m.strip() for m in args.modules.split(",") if m.strip()]
    scopes = [glob_to_regex(g) for group in args.scope for g in group]
    excludes = [glob_to_regex(g) for group in args.exclude for g in group]

    def in_scope(path):
        if scopes and not path_matches(path, scopes):
            return False
        return not path_matches(path, excludes)

    index = JavaIndex()
    for m in modules:
        if not index.add_module(m):
            parser.error("no " + TEST_ROOT + " under module " + m)
    if "athena-federation-sdk" not in modules:
        index.add_module("athena-federation-sdk")

    all_tests = {}
    scoped = OrderedDict()
    for m in modules:
        for key, path in index.collect(m).items():
            all_tests[key] = (m, path)
            if in_scope(path):
                scoped[key] = m

    other_modules = [d for d in sorted(os.listdir(REPO)) if d not in modules and os.path.isdir(os.path.join(REPO, d, TEST_ROOT))]

    findings = []

    def add(kind, module, line, subject, detail=""):
        findings.append((kind, module, str(line) if line else "-", subject, detail))

    rows = []
    for lineno, cols in read_ledger():
        if len(cols) != 4:
            if len(cols) < 4:
                cols = cols + [""] * (4 - len(cols))
            add("malformed_row", "-", lineno, cols[0], "expected 4 tab-separated columns")
        java_test, rust_test, status, note = [c.strip() for c in cols[:4]]
        if java_test in all_tests:
            module, path = all_tests[java_test]
            if java_test not in scoped:
                continue
        else:
            path = fq_to_path(java_test.split("#", 1)[0])
            module = None
            for m in modules:
                if os.path.isfile(os.path.join(REPO, m, TEST_ROOT, path)):
                    module = m
                    break
            if module is None:
                if any(os.path.isfile(os.path.join(REPO, d, TEST_ROOT, path)) for d in other_modules):
                    continue
                module = modules[0] if len(modules) == 1 else "-"
            if not in_scope(path):
                continue
            add("unknown_java_test", module, lineno, java_test, "no Java test method with this id")
        rows.append((lineno, java_test, rust_test, status, note, module))

    by_java = defaultdict(list)
    by_rust = defaultdict(list)
    for lineno, java_test, rust_test, status, note, module in rows:
        by_java[java_test].append((lineno, module))
        if rust_test:
            by_rust[rust_test].append((lineno, module))
    for kind, groups in (("duplicate_java_test", by_java), ("duplicate_rust_test", by_rust)):
        for key, hits in groups.items():
            if len(hits) > 1:
                add(kind, hits[0][1], hits[1][0], key, "lines " + ",".join(str(h[0]) for h in hits))

    covered = set(r[1] for r in rows)
    for key, module in scoped.items():
        if key not in covered:
            add("missing_ledger_row", module, None, key, all_tests[key][1])

    for lineno, java_test, rust_test, status, note, module in rows:
        if status not in VALID_STATUS:
            add("bad_status", module, lineno, java_test, "status '" + status + "'")
        if status == "adapted" and not note:
            add("adapted_without_note", module, lineno, java_test, "")
        if not rust_test:
            add("missing_rust_test", module, lineno, java_test, "empty rust_test")

    results = None
    mode = "skipped"
    if not args.skip_cargo:
        results, build_errors, mode = cargo_results()
        for e in build_errors:
            add("cargo_error", "-", None, "cargo test", e)
        if results is not None:
            for lineno, java_test, rust_test, status, note, module in rows:
                if not rust_test:
                    continue
                res = results.get(rust_test)
                if res is None:
                    add("missing_rust_test", module, lineno, rust_test, "not in cargo test results for " + java_test)
                elif res == "FAILED":
                    add("rust_test_failed", module, lineno, rust_test, java_test)
                elif res == "ignored":
                    add("rust_test_ignored", module, lineno, rust_test, java_test)

    for f in findings:
        print("FINDING\t" + "\t".join(f))
    for m in modules:
        java_n = sum(1 for v in scoped.values() if v == m)
        mod_rows = [r for r in rows if r[5] == m]
        if results is None:
            passing = "n/a"
        else:
            passing = str(sum(1 for r in mod_rows if results.get(r[2]) == "ok"))
        print("SUMMARY\t%s\tjava_tests=%d\tledger_rows=%d\tpassing=%s\tcargo=%s" % (m, java_n, len(mod_rows), passing, mode))
    print("RESULT\t%s\tfindings=%d" % ("PASS" if not findings else "FAIL", len(findings)))
    return 0 if not findings else 1


if __name__ == "__main__":
    sys.exit(main())
