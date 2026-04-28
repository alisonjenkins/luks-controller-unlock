#!/usr/bin/env bash
# Dracut module: ship the controller-unlock agent into initrd.
# Install with:  cp -r dist/dracut/90luks-controller-unlock /usr/lib/dracut/modules.d/
# Then regenerate:  dracut --force

check() {
    require_binaries luks-controller-unlock || return 1
    require_binaries cryptsetup || return 1
    return 0
}

depends() {
    echo "crypt systemd"
    return 0
}

installkernel() {
    # Pull in modules required for DRM rendering and HID controllers
    # in the initrd. Without these, the agent has nothing to draw on
    # and no controller to read from.
    instmods drm_kms_helper drm
    instmods evdev hid hid-generic
    # Vendor HID drivers — covers Xbox via xpad, PlayStation 4/5 via
    # hid-sony / hid-playstation, Switch Pro via hid-nintendo, and
    # Steam Controller via hid-steam.
    instmods xpad hid-sony hid-playstation hid-nintendo hid-steam
    # GPU drivers commonly needed at boot. Add others as required.
    instmods amdgpu i915 nouveau radeon
}

install() {
    inst_binary luks-controller-unlock
    inst_binary cryptsetup
    inst_simple "${moddir}/luks-controller-unlock.service" \
        "${systemdsystemunitdir}/luks-controller-unlock.service"
    $SYSTEMCTL -q --root "$initdir" enable luks-controller-unlock.service
}
