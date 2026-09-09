#!/bin/sh
# Wrapper utilise par geckodriver comme "binaire Firefox".
# Firefox est installe via Flatpak : /usr/bin/firefox n existe pas.
# geckodriver appelle ce script avec ses propres arguments
# (-marionette, --remote-debugging-port, -profile, --kiosk, ...).
exec /usr/bin/flatpak run org.mozilla.firefox "$@"
