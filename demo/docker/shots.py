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
    wait.until(text_present("3DBenchy"))
    wait.until_not(text_present("0 objects across 0 folders"))

    # Open every folder in the sidebar so the shot shows the nesting, not five
    # collapsed roots. Each pass reveals the next level down.
    for _ in range(4):
        chevrons = drv.find_elements(By.CSS_SELECTOR, "[aria-label='Expand']")
        if not chevrons:
            break
        for c in chevrons:
            try:
                c.click()
            except Exception:
                pass  # a re-render can detach the node mid-loop; next pass gets it
        time.sleep(0.3)

    # Expanding scrolls the tree; put it back at the top so the roots are in frame.
    home = drv.find_element(By.XPATH, "//*[contains(text(),'All objects')]")
    drv.execute_script("arguments[0].scrollIntoView({block: 'start'})", home)

    time.sleep(1.5)  # let the card thumbnails settle
    shot("01-library")

    # Deliberately not the Benchy: its ~11MB STL comes back truncated over the
    # number[] IPC, and because the file's binary header starts with "solid"
    # STLLoader then falls back to its ASCII parser and yields garbage geometry.
    drv.find_element(By.XPATH, "//*[contains(text(),'Mexican skull')]").click()
    wait.until(text_present("Kitchen"))
    time.sleep(6)  # llvmpipe needs a moment for the first WebGL frames
    shot("02-object-detail")

    print("OK")
except Exception as e:
    drv.save_screenshot("/out/99-failure.png")
    print(f"FAIL: {type(e).__name__}: {e}")
    sys.exit(1)
finally:
    drv.quit()
