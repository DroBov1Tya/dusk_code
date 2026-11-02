import sys
sys.path.insert(0, ".")
sys.dont_write_bytecode = True
import os, tempfile
from server import resolve
with tempfile.TemporaryDirectory() as base:
    base = os.path.realpath(base)
    root = os.path.join(base, "data"); os.mkdir(root)
    evil = os.path.join(base, "data-evil"); os.mkdir(evil)
    open(os.path.join(evil, "x"), "w").close()
    open(os.path.join(base, "secret"), "w").close()
    os.mkdir(os.path.join(root, "sub"))
    good = os.path.join(root, "sub", "b.txt"); open(good, "w").close()
    os.symlink(os.path.join(base, "secret"), os.path.join(root, "link"))
    os.symlink(good, os.path.join(root, "inner"))
    os.symlink(base, os.path.join(root, "out"))
    for ok in ["sub/b.txt", "sub/../sub/b.txt", "sub%2Fb.txt", "inner", "/sub/b.txt"]:
        got = resolve(root, ok)
        assert got == good, (ok, got)
    for bad in ["../secret", "sub/../../secret", "%2e%2e/secret", "%2e%2e%2fsecret", "%252e%252e%252fsecret",
                "%25252e%25252e/secret", "..%2fdata-evil/x", "../data-evil/x", "link", "out/secret",
                "sub/%2e%2e/%2e%2e/secret", "a%00/../../secret"]:
        try:
            got = resolve(root, bad)
        except PermissionError:
            continue
        except ValueError as error:
            raise SystemExit(f"{bad!r}: {error!r} is not PermissionError")
        raise SystemExit(f"{bad!r} should be refused, gave {got}")
