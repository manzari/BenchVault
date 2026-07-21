import json, sys, time
from selenium import webdriver
from selenium.webdriver.common.options import ArgOptions

opts = ArgOptions()
opts.set_capability("browserName", "wry")
opts.set_capability("tauri:options", {"application": "/app/benchvault"})

drv = webdriver.Remote(command_executor="http://127.0.0.1:4444", options=opts)
try:
    # The window size comes from tauri.conf.json (1280x800); pinning it here too
    # keeps screenshot dimensions stable if that config ever changes.
    drv.set_window_size(1280, 800)
    time.sleep(4)  # app boot: migrations + startup reconcile before first paint

    info = drv.execute_script(
        "return {title: document.title, url: location.href,"
        " body: (document.body ? document.body.innerText.slice(0,300) : null),"
        " nodes: document.querySelectorAll('*').length}"
    )
    print("APPINFO " + json.dumps(info))
    drv.save_screenshot("/out/tauri-spike.png")
    ok = bool(info and info.get("nodes", 0) > 20)
    print("OK" if ok else "FAIL: webview rendered too few nodes")
    sys.exit(0 if ok else 1)
finally:
    drv.quit()
