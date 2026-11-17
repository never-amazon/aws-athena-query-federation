import re
import sys

PATCHED = re.compile(r"^(.*) \[patched (.*?) => (.*)\]$", re.S)
FETCH = re.compile(r"^(.*) \[(FETCH:[a-z0-9]+)\]$", re.S)


def query_line(table, query):
    fetch = FETCH.match(query)
    if fetch:
        return f"{table}\t{fetch.group(1)}\t{fetch.group(2)}"
    patched = PATCHED.match(query)
    if patched:
        return f"{table}\t{patched.group(1)}\t{patched.group(2)}=>{patched.group(3)}"
    return f"{table}\t{query}"


def main():
    fixture, out = sys.argv[1], sys.argv[2]
    lines = []
    with open(fixture, encoding="utf-8", newline="\n") as f:
        for line in f:
            if line.startswith("QUERY\t"):
                _, table, query = line.rstrip("\n").split("\t", 2)
                lines.append(query_line(table, query))
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write("".join(line + "\n" for line in lines))


if __name__ == "__main__":
    main()
