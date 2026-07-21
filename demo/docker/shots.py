import sys, time
from selenium import webdriver
from selenium.webdriver.common.by import By
from selenium.webdriver.common.options import ArgOptions
from selenium.webdriver.support import expected_conditions as EC
from selenium.webdriver.support.ui import WebDriverWait

opts = ArgOptions()
opts.set_capability("browserName", "wry")
opts.set_capability("tauri:options", {"application": "/app/benchvault"})

drv = webdriver.Remote(command_executor="http://127.0.0.1:4444", options=opts)
wait = WebDriverWait(drv, 20)


def shot(name):
    drv.save_screenshot(f"/out/{name}.png")
    print(f"  captured {name}.png")


# WebKitWebDriver returns "" for element.text here, so read innerText directly.
def text_present(needle):
    return lambda d: needle in (d.execute_script("return document.body.innerText") or "")


try:
    drv.set_window_size(1280, 800)

    # Wait on seeded content rather than sleeping — proves the DB was read.
    wait.until(text_present("Cable Clip"))
    shot("01-library")

    drv.find_element(By.XPATH, "//*[contains(text(),'XYZ Calibration Cube')]").click()
    wait.until(text_present("Calibration"))
    time.sleep(2.5)  # let the WebGL STL preview finish its first frames
    shot("02-object-detail")

    print("OK")
except Exception as e:
    drv.save_screenshot("/out/99-failure.png")
    print(f"FAIL: {type(e).__name__}: {e}")
    sys.exit(1)
finally:
    drv.quit()
