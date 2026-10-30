#!/usr/bin/env bash
set -u
LOG=/tmp/park-gateway-browser-helper.log
{
  date '+%F %T restart browser requested'
  pkill -u kickpi -f 'chromium.*park-gateway-kiosk' 2>/dev/null || true
  sleep 1
  rm -f /home/kickpi/.cache/park-gateway-kiosk/SingletonCookie /home/kickpi/.cache/park-gateway-kiosk/SingletonLock /home/kickpi/.cache/park-gateway-kiosk/SingletonSocket 2>/dev/null || true
  rm -f /tmp/park-gateway-kiosk-restart.log 2>/dev/null || true
  sudo -u kickpi env DISPLAY=:0 XAUTHORITY=/home/kickpi/.Xauthority XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus nohup /usr/local/bin/park-gateway-kiosk >/tmp/park-gateway-kiosk-restart.log 2>&1 &
} >>"$LOG" 2>&1
chmod 0666 "$LOG" 2>/dev/null || true
