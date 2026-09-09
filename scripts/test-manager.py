"""HTTP regression checks against an isolated Manager with 200 kiosks.

Usage: python scripts/test-manager.py path/to/noc-manager.exe
Only creates and changes data inside a new temporary test directory.
"""
import json
import pathlib
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request


def main():
    binary = pathlib.Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="noc-manager-test-") as directory:
        root = pathlib.Path(directory).resolve()
        assert root.parent == pathlib.Path(tempfile.gettempdir()).resolve()
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        base = f"http://127.0.0.1:{port}"
        (root / "config.toml").write_text(f'[server]\nlisten="127.0.0.1"\nport={port}\napi_token="test-token"\n', encoding="utf-8")
        fixtures = [dict(username=f"a-{i}", display_username=f"d-{i}", name=f"Kiosk {i}", url="https://example.com", enabled=True) for i in range(200)]
        fixtures[0].pop("display_username")  # old format remains supported
        (root / "kiosks.json").write_text(json.dumps(fixtures), encoding="utf-8")
        log = (root / "server.log").open("w")

        def start():
            process = subprocess.Popen([str(binary)], cwd=root, stdout=log, stderr=log, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            for _ in range(100):
                try:
                    with urllib.request.urlopen(base + "/", timeout=1):
                        return process
                except urllib.error.URLError:
                    if process.poll() is not None:
                        raise RuntimeError((root / "server.log").read_text())
                    time.sleep(.1)
            process.terminate()
            process.wait()
            raise RuntimeError("Manager startup timeout")

        def call(path, data=None, form=False, auth=True, expected=200):
            headers = {"Authorization": "Bearer test-token"} if auth else {}
            if data is not None:
                data = (urllib.parse.urlencode(data) if form else json.dumps(data)).encode()
                headers["Content-Type"] = "application/x-www-form-urlencoded" if form else "application/json"
            request = urllib.request.Request(base + path, data=data, headers=headers)
            try:
                with urllib.request.urlopen(request, timeout=10) as response:
                    code, raw = response.status, response.read().decode()
            except urllib.error.HTTPError as response:
                code, raw = response.code, response.read().decode()
            assert code == expected, (path, code, raw[:200])
            try:
                return json.loads(raw)
            except ValueError:
                return raw

        process = start()
        try:
            call("/api/health", auth=False, expected=401)
            for route in ("/", "/kiosks", "/supervision", "/kiosk/new", "/rdp-servers", "/rdp-servers/new"):
                assert 'lang="fr"' in call(route)
            assert "refresh" in call("/assets/manager.js")
            assert "--accent" in call("/assets/manager.css")
            inventory = call("/ui/status", auth=False)
            assert len(inventory["kiosks"]) == 200
            assert inventory["clients"] == []
            assert call("/api/kiosk/a-0/rdp")["username"] == "a-0"
            prefill = call("/kiosk/a-0/edit?agent=replacement-agent")
            assert 'name="username" value="replacement-agent"' in prefill
            assert 'name="display_username" value="a-0"' in prefill
            assert call("/api/kiosk/d-1/rdp")["username"] == "a-1"
            call("/api/kiosk/a-1/rdp", expected=404)
            call("/api/heartbeat/invalid/a", {}, expected=400)
            for i in range(200):
                for app, user, state in (("display", "a-0" if i == 0 else f"d-{i}", "CONNECTED"), ("agent", f"a-{i}", "RUNNING")):
                    call(f"/api/heartbeat/{app}/{user}", dict(version="test", build="fixture", state=state))
            inventory = call("/ui/status")
            assert len(inventory["clients"]) == 400
            assert all(k["status"] == "ready" for k in inventory["kiosks"])
            call("/api/heartbeat/display/new-display", dict(state="STARTING"))
            assert len([c for c in call("/ui/status")["clients"] if c["kiosk"] is None]) == 1
            prefill = call("/kiosk/new?display=new-display")
            for field in ("name", "display_username"):
                assert f'name="{field}" value="new-display"' in prefill
            assert 'name="username" value=""' in prefill  # Linux account is entered manually
            assert 'id="rdp-server-select"' in prefill
            assert 'id="inline-server-save"' in prefill
            assert 'name="rdp_enabled" value="1" checked' in prefill
            assert 'name="rdp_port" value="3389"' in prefill
            assert 'name="restart_cron" value=""' in prefill
            assert "À compléter" in prefill
            missing = dict(username="new-agent", display_username="new-display", name="new-display", url="https://example.com", enabled="1", rdp_enabled="1", rdp_server="linux.example", rdp_port="3389")
            assert "Saisissez le mot de passe RDP" in call("/kiosk/new", missing, form=True)
            call("/api/kiosk/new-agent", expected=404)
            assert "L’URL est obligatoire" in call("/kiosk/new", dict(missing, url="", rdp_use_local="1"), form=True)
            assert "indiquez un serveur" in call("/kiosk/new", dict(missing, rdp_server="", rdp_use_local="1"), form=True)
            destination = dict(name="Linux principal", address="linux.example", port="3391")
            server = call("/ui/rdp-servers", destination, form=True)
            assert server["address"] == "linux.example" and server["port"] == 3391
            assert "password" not in server and "username" not in server
            call("/ui/rdp-servers", destination, form=True, expected=400)
            call("/ui/rdp-servers", dict(name="Invalid",address="https://example.com",port="3389"), form=True, expected=400)
            call("/ui/rdp-servers", dict(name="Invalid",address="linux.example",port="70000"), form=True, expected=400)
            assert "Linux principal" in call("/rdp-servers")
            assert "Linux principal" in call("/kiosk/new?display=new-display")
            assert "n’existe plus" in call("/kiosk/new", dict(missing, rdp_server_id="nonexistent",rdp_use_local="1"),form=True)
            form = dict(username="new-agent", display_username="new-display", name="New <kiosk>", url="https://example.com", enabled="1", rdp_enabled="1", rdp_server_id=server["id"], rdp_server="ignored.example", rdp_port="1234", rdp_password="test-only-secret")
            call("/kiosk/new", form, form=True)
            assert call("/api/kiosk/new-display/rdp")["username"] == "new-agent"
            assert call("/api/kiosk/new-display/rdp")["password"] == "test-only-secret"
            assert call("/api/kiosk/new-display/rdp")["server"] == "linux.example"
            assert call("/api/kiosk/new-display/rdp")["port"] == 3391
            assert f'value="{server["id"]}" selected' in call("/kiosk/new-agent/edit")
            call(f'/rdp-servers/{server["id"]}/edit',dict(destination,address="linux-updated.example",port="3392",ignore_certificate_errors="1"),form=True)
            remote = call("/api/kiosk/new-display/rdp")
            assert remote["server"] == "linux-updated.example" and remote["port"] == 3392
            assert remote["ignore_certificate_errors"] and remote["username"] == "new-agent"
            assert remote["password"] == "test-only-secret"
            assert 'action="/kiosk/new-agent/edit"' in call("/kiosk/new?display=new-display")
            assert not any(c["username"] == "new-agent" for c in call("/ui/status")["clients"])
            call("/api/heartbeat/display/new-display", dict(state="CONNECTED"))
            call("/api/heartbeat/agent/new-agent", dict(state="RUNNING"))
            assert next(k for k in call("/ui/status")["kiosks"] if k["username"] == "new-agent")["status"] == "ready"
            local = dict(missing, username="local-display", display_username="local-display", rdp_use_local="1")
            call("/kiosk/new", local, form=True)
            assert call("/api/kiosk/local-display/rdp")["password"] is None
            assert call("/api/kiosk/local-display/rdp")["username"] == "local-display"
            call("/kiosk/local-display/delete", {}, form=True)
            for route in ("/ui/status", "/ui/history", "/api/kiosk/new-agent", "/kiosk/new-agent/edit", "/metrics"):
                assert "test-only-secret" not in str(call(route)), route
            form["rdp_password"] = ""
            call("/kiosk/new-agent/edit", form, form=True)
            assert call("/api/kiosk/new-display/rdp")["password"] == "test-only-secret"
            assert "New &lt;kiosk&gt;" in call("/kiosk/new-agent/edit")
            conflicting = dict(form, username="conflict-agent", rdp_use_local="1")
            assert "déjà associé" in call("/kiosk/new", conflicting, form=True)
            call("/api/kiosk/conflict-agent", expected=404)
            call("/kiosk/new-agent/restart-browser", {}, form=True)
            command = call("/api/kiosk/new-agent/command")["command"]
            assert command["action"] == "restart_browser"
            call(f'/api/kiosk/new-agent/command/{command["id"]}/ack', {})
            assert call("/api/kiosk/new-agent/command")["command"] is None
            all_ids, before = set(), None
            while True:
                page = call("/ui/history" + (f"?before={before}" if before else ""))
                ids = {e["id"] for e in page["events"]}
                assert not all_ids.intersection(ids)
                assert len(page["events"]) <= 100
                all_ids.update(ids)
                before = page["next"]
                if before is None:
                    break
            assert len(all_ids) == 403
            events = call("/ui/history?app=agent&username=new-agent&state=RUNNING")["events"]
            assert len(events) == 1
            call("/ui/history?from=9&to=1", expected=400)
            call("/kiosk/new-agent/delete", {}, form=True)
            assert len(call("/ui/history?username=new-agent")["events"]) == 1
            assert len([c for c in call("/ui/status")["clients"] if c["kiosk"] is None]) == 2
            call("/kiosk/new", dict(missing, username="persistent-agent", display_username="persistent-display", rdp_server_id=server["id"], rdp_use_local="1"), form=True)
            process.terminate()
            process.wait(timeout=10)
            process = start()
            assert "linux-updated.example:3392" in call("/rdp-servers")
            assert f'value="{server["id"]}"' in call("/kiosk/new")
            assert call("/api/kiosk/persistent-display/rdp")["server"] == "linux-updated.example"
            assert call("/api/kiosk/persistent-display/rdp")["username"] == "persistent-agent"
            assert len(call("/ui/status")["clients"]) == 402
            assert len(call("/ui/history?username=new-agent")["events"]) == 1
            print("PASS: 200 kiosks / 402 clients, display discovery, manual Linux accounts, RDP catalogue creation/update/selection, legacy configs, password isolation, commands, history and restart.")
        finally:
            process.terminate()
            process.wait(timeout=10)
            log.close()


if __name__ == "__main__":
    main()
