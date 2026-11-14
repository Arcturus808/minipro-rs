#!/bin/sh
# Package postinst: activate the installed 60-minipro-rs.rules so the
# programmer is usable without a reboot. Tolerates non-systemd/minimal
# environments — failing udevadm calls never fail the install.
udevadm control --reload-rules 2>/dev/null || true
udevadm trigger --subsystem-match=usb 2>/dev/null || true
