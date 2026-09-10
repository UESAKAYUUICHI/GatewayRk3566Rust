#!/usr/bin/env bash
# RK3568 板端安装脚本（在设备上以 root 运行；配合 .deb 或手工拷贝产物）
set -euo pipefail

if command -v dpkg >/dev/null 2>&1 && ls park-gateway_*.deb >/dev/null 2>&1; then
    dpkg -i park-gateway_*.deb
else
    echo "未找到 .deb，执行手工安装…"
    install -m 0755 gateway-web /usr/bin/gateway-web
    install -m 0755 gateway-headless /usr/bin/gateway-headless || true
    install -m 0755 park-gateway /usr/bin/park-gateway || true
    mkdir -p /etc/park-gateway /var/lib/park-gateway /usr/share/park-gateway/profiles /usr/share/park-gateway/web-ui
    install -m 0644 gateway.toml /etc/park-gateway/gateway.toml
    install -m 0644 profiles/*.toml /usr/share/park-gateway/profiles/
    if [ -d web-ui ]; then
        cp -a web-ui/. /usr/share/park-gateway/web-ui/
    elif [ -d ../web-ui/dist ]; then
        cp -a ../web-ui/dist/. /usr/share/park-gateway/web-ui/
    fi
    getent group netdev >/dev/null 2>&1 || groupadd -r netdev
    id parkgw >/dev/null 2>&1 || useradd -r -s /usr/sbin/nologin -G dialout,netdev parkgw
    usermod -aG dialout,netdev parkgw
    chown -R parkgw:dialout /var/lib/park-gateway
    install -m 0644 park-gateway.service /etc/systemd/system/
    install -m 0644 60-park-gateway-serial.rules /etc/udev/rules.d/
    if [ -f park-gateway-kiosk ]; then
        install -m 0755 park-gateway-kiosk /usr/local/bin/park-gateway-kiosk
        install -d -m 0755 -o kickpi -g kickpi /home/kickpi/.config/autostart /home/kickpi/.cache/park-gateway-kiosk
        install -m 0644 -o kickpi -g kickpi park-gateway-kiosk.desktop /home/kickpi/.config/autostart/park-gateway-kiosk.desktop
        install -d -m 0755 /etc/lightdm/lightdm.conf.d
        cat >/etc/lightdm/lightdm.conf.d/50-park-gateway-autologin.conf <<'CONF'
[Seat:*]
autologin-user=kickpi
autologin-user-timeout=0
user-session=xfce
CONF
    fi
    if [ -d /etc/polkit-1/rules.d ]; then
        install -m 0644 49-park-gateway-networkmanager.rules /etc/polkit-1/rules.d/
        install -m 0644 50-park-gateway-power.rules /etc/polkit-1/rules.d/
    fi
    udevadm control --reload && udevadm trigger
fi

systemctl daemon-reload
systemctl enable --now park-gateway.service
systemctl --no-pager status park-gateway.service | head -12 || true
echo "查看 Web UI：http://<网关IP>:8080/ 。日志：journalctl -u park-gateway -f"
