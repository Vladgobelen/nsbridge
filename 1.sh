#!/bin/bash
# watch-tray.sh — наблюдение за SNI-треем в реальном времени.
# Использование: ./watch-tray.sh
# Запусти, потом стартуй/закрывай qBittorrent, смотри вывод. Ctrl-C — стоп.

set -u

SOCK="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/bus"

echo "=== watch-tray: $(date) ==="
echo "Следим:"
echo "  - org.kde.StatusNotifierWatcher (Registered/Unregistered)"
echo "  - org.kde.StatusNotifierItem      (NewTitle/NewIcon/NewStatus)"
echo "  - org.freedesktop.DBus            (NameOwnerChanged)"
echo "  - RegisteredStatusNotifierItems раз в секунду"
echo "  - pgrep qbittorrent раз в секунду"
echo ""

# 1. Фоновый dbus-monitor с таймстампами.
dbus-monitor --session \
    "type='signal',interface='org.kde.StatusNotifierWatcher'" \
    "type='signal',interface='org.kde.StatusNotifierItem'" \
    "type='signal',interface='org.freedesktop.DBus',member='NameOwnerChanged'" \
    2>&1 | while IFS= read -r line; do
        printf '[%s] %s\n' "$(date +%H:%M:%S.%3N)" "$line"
    done &

MON_PID=$!
trap 'kill $MON_PID 2>/dev/null; exit 0' INT TERM

# 2. Раз в секунду: SNI-список + pgrep qbittorrent.
prev_sni=""
prev_qbt=""
while true; do
    sni=$(busctl --user call org.kde.StatusNotifierWatcher /StatusNotifierWatcher \
        org.freedesktop.DBus.Properties Get ss \
        org.kde.StatusNotifierWatcher RegisteredStatusNotifierItems 2>/dev/null \
        | tr -d '\n')

    qbt=$(pgrep -x qbittorrent >/dev/null 2>&1 && echo "UP" || echo "down")

    if [ "$sni" != "$prev_sni" ] || [ "$qbt" != "$prev_qbt" ]; then
        printf '[%s] STATE sni=%s | qbittorrent=%s\n' \
            "$(date +%H:%M:%S.%3N)" "$sni" "$qbt"
        prev_sni="$sni"
        prev_qbt="$qbt"
    fi
    sleep 1
done