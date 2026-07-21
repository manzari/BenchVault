# tauri-driver is built on an older glibc than the runtime stage on purpose:
# old-glibc binaries run on newer, not the reverse.
FROM rust:1-slim AS driver
RUN cargo install tauri-driver --version 2.0.6 --locked --root /out

# Headless runtime for demo screenshots. Based on the same Ubuntu release as the
# dev host so a host-built binary (glibc 2.43) runs here unchanged — rebasing
# this image on an older LTS breaks with "GLIBC_2.43 not found".
FROM ubuntu:26.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
      libwebkit2gtk-4.1-0 \
      webkitgtk-webdriver \
      xvfb \
      xauth \
      x11-utils \
      mesa-utils \
      libgl1-mesa-dri \
      libglx-mesa0 \
      libegl-mesa0 \
      libgtk-3-0 \
      libayatana-appindicator3-1 \
      fonts-dejavu-core \
      python3 \
      python3-selenium \
      ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=driver /out/bin/tauri-driver /usr/local/bin/tauri-driver

WORKDIR /spike
COPY webgl.html spike.py tauri_spike.py shots.py ./
COPY run-webgl-spike.sh run-tauri-spike.sh run-shots.sh ./
RUN chmod +x run-webgl-spike.sh run-tauri-spike.sh run-shots.sh

ENTRYPOINT ["/spike/run-webgl-spike.sh"]
