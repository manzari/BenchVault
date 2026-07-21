import json, sys, time
from selenium import webdriver
from selenium.webdriver.common.options import ArgOptions

opts = ArgOptions()
opts.set_capability("browserName", "MiniBrowser")

drv = webdriver.Remote(command_executor="http://127.0.0.1:4444", options=opts)
try:
    drv.set_window_size(1280, 800)
    drv.get("file:///spike/webgl.html")
    time.sleep(1.5)
    res = drv.execute_script("return window.__spike")
    print("RESULT " + json.dumps(res))
    drv.save_screenshot("/out/webgl-spike.png")
    sys.exit(0 if res and res.get("ok") else 1)
finally:
    drv.quit()
