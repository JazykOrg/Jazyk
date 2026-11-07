#!/usr/bin/env python3
"""Exercise editor setup with fake IDEs and an isolated home, without model calls."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

binary = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix="jazyk editor launcher ") as temporary:
    base = Path(temporary).resolve()
    home, project, programs = (base / name for name in ("home", "project", "bin"))
    for directory in (home, project / ".jazyk", programs):
        directory.mkdir(parents=True)
    (project / ".jazyk/jazyk.toml").write_text("version = 1\nsets = []\n")
    launcher = '''#!/usr/bin/python3
import json, os, pathlib, sys
home = pathlib.Path(os.environ["HOME"])
if sys.argv[1:2] == ["installPlugins"]:
    properties = pathlib.Path(os.environ["TESTIDE_PROPERTIES"])
    (home / "installer-properties.txt").write_text(properties.read_text())
    values = dict(line.split("=", 1) for line in properties.read_text().splitlines())
    plugins = values["idea.plugins.path"].encode("ascii").decode("unicode_escape")
    (pathlib.Path(plugins) / "lsp4ij").mkdir(parents=True, exist_ok=True)
else:
    (home / "launched.json").write_text(json.dumps(sys.argv[1:]))
'''
    zed = programs / "zed"
    zed.write_text(launcher)
    zed.chmod(0o755)
    env = os.environ | {"HOME": str(home), "PATH": f"{programs}:/usr/bin:/bin"}
    # Keep platform data roots isolated even when the parent defines XDG overrides.
    env |= {"XDG_DATA_HOME": str(home / "data"), "APPDATA": str(home / "roaming"),
            "LOCALAPPDATA": str(home / "local")}

    def launch(choice):
        result = subprocess.run([str(binary), choice], cwd=project, env=env,
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60)
        if result.returncode:
            raise AssertionError(f"{choice}: {result.stdout}\n{result.stderr}")
        assert json.loads((home / "launched.json").read_text()) == [str(project)]
        return result.stdout

    first = launch("zed")
    settings = project / ".zed/settings.json"
    text = settings.read_text()
    config = json.loads(text)
    assert config["languages"]["Markdown"]["language_servers"] == ["jazyk", "..."]
    assert Path(config["lsp"]["jazyk"]["binary"]["path"]).resolve() == binary
    assert "installed Zed LSP adapter" in first, first
    launch("zed")
    assert settings.read_text() == text
    print("Zed: launcher, adapter installation, project settings, repeat setup passed")

    for choice in ("idea", "phpstorm", "webstorm", "pycharm", "goland", "rustrover", "clion", "rider", "rubymine"):
        product = base / "products" / choice
        (product / "bin").mkdir(parents=True)
        executable = product / "bin" / choice
        executable.write_text(launcher)
        executable.chmod(0o755)
        (programs / choice).symlink_to(executable)
        (product / "product-info.json").write_text(json.dumps({
            "dataDirectoryName": f"JazykTest-{choice}", "envVarBaseName": "TESTIDE",
            "launch": [{"os": "macOS" if sys.platform == "darwin" else "Linux",
                        "launcherPath": f"bin/{choice}"}]}))
        output = launch(choice)
        assert "installed JetBrains LSP adapter" in output, output
        assert "installing the free LSP4IJ client" in output, output
        properties = (home / "installer-properties.txt").read_text()
        assert "idea.config.path=" in properties and "idea.system.path=" in properties
        config = json.loads((project / ".jazyk/ide.json").read_text())
        assert Path(config["lsp"]["command"]).resolve() == binary
        output = launch(choice)
        assert "installing the free LSP4IJ client" not in output, output
        assert "installed JetBrains LSP adapter" not in output, output
        print(f"{choice}: adapter installation, isolated dependency installer, executable, repeat setup passed")
print("All editor launcher smoke checks passed")
