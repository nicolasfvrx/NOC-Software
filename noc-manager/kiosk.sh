#!/usr/bin/env bash
#
# kiosk.sh - KioskManager client
#
# Runs inside each Linux xRDP session, keeps Firefox displaying the URL
# configured in KioskManager, and restarts it on the configured cron schedule.
#
# Dependencies: bash, curl, jq, firefox
#   sudo apt update && sudo apt install curl jq firefox -y
#
# ---------------------------------------------------------------- settings ---

KIOSK_SERVER="http://192.168.10.64:8080"

# Optional: must match [server] api_token in config.toml. Empty = no auth.
KIOSK_API_TOKEN=""

POLL_INTERVAL=30     # seconds between configuration refreshes
RETRY_INTERVAL=10    # seconds before retrying after an API error
RESTART_DELAY=3      # seconds to wait between stopping and starting Firefox
STOP_TIMEOUT=10      # seconds to wait for a clean Firefox exit before killing
TICK=5               # main loop resolution

# ------------------------------------------------------------------ runtime ---

USERNAME="$(id -un)"

CUR_URL=""
CUR_ENABLED=""
CUR_CRON=""
NEW_URL=""
NEW_ENABLED=""
NEW_CRON=""
CONFIG_OK=0
FIREFOX_PID=""
LAST_CRON_TRIGGER=""
LAST_POLL=0

log() {
    printf '%s [%s] %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$USERNAME" "$*"
}

require() {
    command -v "$1" >/dev/null 2>&1 || {
        log "missing dependency: $1"
        exit 1
    }
}

# ------------------------------------------------------------------ firefox ---

firefox_running() {
    [ -n "$FIREFOX_PID" ] && kill -0 "$FIREFOX_PID" 2>/dev/null
}

start_firefox() {
    [ -n "$CUR_URL" ] || return 0
    log "launching Firefox: $CUR_URL"
    firefox --kiosk "$CUR_URL" >/dev/null 2>&1 &
    FIREFOX_PID=$!
}

stop_firefox() {
    firefox_running || {
        FIREFOX_PID=""
        return 0
    }

    log "stopping Firefox (pid $FIREFOX_PID)"
    kill -TERM "$FIREFOX_PID" 2>/dev/null

    local waited=0
    while [ "$waited" -lt "$STOP_TIMEOUT" ]; do
        kill -0 "$FIREFOX_PID" 2>/dev/null || break
        sleep 1
        waited=$((waited + 1))
    done

    # Forced kill only as a fallback.
    if kill -0 "$FIREFOX_PID" 2>/dev/null; then
        log "Firefox did not exit cleanly, forcing"
        kill -KILL "$FIREFOX_PID" 2>/dev/null
        sleep 1
    fi

    wait "$FIREFOX_PID" 2>/dev/null
    FIREFOX_PID=""
}

# ---------------------------------------------------------------------- api ---

# Fills NEW_ENABLED / NEW_URL / NEW_CRON.
# Returns 0 on success, 1 on transport error, 2 when the kiosk is unknown.
fetch_config() {
    local args=(-sS --max-time 10 -w '\n%{http_code}')
    [ -n "$KIOSK_API_TOKEN" ] && args+=(-H "Authorization: Bearer $KIOSK_API_TOKEN")

    local out body code
    out="$(curl "${args[@]}" "$KIOSK_SERVER/api/kiosk/$USERNAME" 2>/dev/null)" || return 1

    code="${out##*$'\n'}"
    body="${out%$'\n'*}"

    [ "$code" = "404" ] && return 2
    [ "$code" = "200" ] || return 1

    printf '%s' "$body" | jq -e . >/dev/null 2>&1 || return 1

    NEW_ENABLED="$(printf '%s' "$body" | jq -r '.enabled // false')"
    NEW_URL="$(printf '%s' "$body" | jq -r '.url // ""')"
    NEW_CRON="$(printf '%s' "$body" | jq -r '.restart_cron // ""')"
    return 0
}

# Applies NEW_* over CUR_*, restarting Firefox only when something changed.
apply_config() {
    local restart_needed=0

    if [ "$CONFIG_OK" -eq 0 ]; then
        CONFIG_OK=1
        log "configuration loaded"
    fi

    if [ "$NEW_ENABLED" != "$CUR_ENABLED" ]; then
        if [ "$NEW_ENABLED" = "true" ]; then
            [ -n "$CUR_ENABLED" ] && log "kiosk enabled"
        else
            log "kiosk disabled"
        fi
    fi

    if [ -n "$CUR_URL" ] && [ "$NEW_URL" != "$CUR_URL" ]; then
        log "URL changed"
        restart_needed=1
    fi

    if [ "$NEW_CRON" != "$CUR_CRON" ]; then
        log "restart schedule: ${NEW_CRON:-none}"
    fi

    CUR_ENABLED="$NEW_ENABLED"
    CUR_URL="$NEW_URL"
    CUR_CRON="$NEW_CRON"

    if [ "$CUR_ENABLED" != "true" ]; then
        stop_firefox
        return 0
    fi

    if [ "$restart_needed" -eq 1 ] && firefox_running; then
        stop_firefox
        sleep "$RESTART_DELAY"
    fi
}

reload_config() {
    if fetch_config; then
        apply_config
        return 0
    fi
    return 1
}

# --------------------------------------------------------------------- cron ---

# cron_field_match <value> <field> <min> <max>
cron_field_match() {
    local value="$1" field="$2" min="$3" max="$4"
    local parts part base step start end

    [ "$field" = "*" ] && return 0

    IFS=',' read -ra parts <<< "$field"
    for part in "${parts[@]}"; do
        step=1
        base="$part"
        case "$part" in
            */*)
                base="${part%%/*}"
                step="${part##*/}"
                ;;
        esac
        [[ "$step" =~ ^[0-9]+$ ]] || return 1
        [ "$step" -gt 0 ] || return 1

        case "$base" in
            '*')   start="$min"; end="$max" ;;
            *-*)   start="${base%%-*}"; end="${base##*-}" ;;
            *)     start="$base"; end="$base" ;;
        esac

        [[ "$start" =~ ^[0-9]+$ && "$end" =~ ^[0-9]+$ ]] || return 1
        start=$((10#$start))
        end=$((10#$end))

        if [ "$value" -ge "$start" ] && [ "$value" -le "$end" ] \
           && [ $(((value - start) % step)) -eq 0 ]; then
            return 0
        fi
    done

    return 1
}

# cron_matches <expression> - true when the current minute matches.
cron_matches() {
    local f_min f_hour f_dom f_mon f_dow extra
    read -r f_min f_hour f_dom f_mon f_dow extra <<< "$1"
    [ -n "$f_dow" ] || return 1
    [ -z "$extra" ] || return 1

    local n_min n_hour n_dom n_mon n_dow
    n_min=$((10#$(date +%M)))
    n_hour=$((10#$(date +%H)))
    n_dom=$((10#$(date +%d)))
    n_mon=$((10#$(date +%m)))
    n_dow=$((10#$(date +%w)))

    cron_field_match "$n_min" "$f_min" 0 59 || return 1
    cron_field_match "$n_hour" "$f_hour" 0 23 || return 1
    cron_field_match "$n_mon" "$f_mon" 1 12 || return 1

    # Sunday is both 0 and 7 in cron syntax.
    local dow_ok=1
    cron_field_match "$n_dow" "$f_dow" 0 6 || dow_ok=0
    if [ "$dow_ok" -eq 0 ] && [ "$n_dow" -eq 0 ]; then
        cron_field_match 7 "$f_dow" 0 7 && dow_ok=1
    fi

    local dom_ok=1
    cron_field_match "$n_dom" "$f_dom" 1 31 || dom_ok=0

    # Standard cron: day-of-month and day-of-week are OR'ed when both are set.
    if [ "$f_dom" = "*" ] || [ "$f_dow" = "*" ]; then
        [ "$dom_ok" -eq 1 ] && [ "$dow_ok" -eq 1 ]
    else
        [ "$dom_ok" -eq 1 ] || [ "$dow_ok" -eq 1 ]
    fi
}

check_cron_restart() {
    [ -n "$CUR_CRON" ] || return 0
    [ "$CONFIG_OK" -eq 1 ] || return 0

    local stamp
    stamp="$(date '+%Y-%m-%d %H:%M')"

    # One restart per matching minute.
    [ "$stamp" = "$LAST_CRON_TRIGGER" ] && return 0
    cron_matches "$CUR_CRON" || return 0

    LAST_CRON_TRIGGER="$stamp"
    log "cron restart triggered"

    stop_firefox
    sleep "$RESTART_DELAY"
    reload_config || log "API unavailable, keeping previous configuration"
}

# --------------------------------------------------------------------- main ---

require curl
require jq
require firefox

trap 'log "stopping"; stop_firefox; exit 0' INT TERM

log "starting, server $KIOSK_SERVER"

while true; do
    # Firefox died or was closed by the user.
    if [ -n "$FIREFOX_PID" ] && ! firefox_running; then
        log "Firefox exited"
        FIREFOX_PID=""
        sleep "$RESTART_DELAY"
        reload_config || log "API unavailable, keeping previous configuration"
    fi

    now=$(date +%s)
    if [ "$CONFIG_OK" -eq 0 ] || [ $((now - LAST_POLL)) -ge "$POLL_INTERVAL" ]; then
        fetch_config
        rc=$?
        if [ "$rc" -eq 0 ]; then
            LAST_POLL="$now"
            apply_config
        else
            if [ "$rc" -eq 2 ]; then
                log "kiosk not found on server, retrying"
            else
                log "API unavailable, retrying"
            fi
            sleep "$RETRY_INTERVAL"
            continue
        fi
    fi

    check_cron_restart

    if [ "$CUR_ENABLED" = "true" ] && ! firefox_running; then
        start_firefox
    fi

    sleep "$TICK"
done
