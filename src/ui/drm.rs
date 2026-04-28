//! DRM/KMS dumb buffer surface. Opens the card, finds the first connected
//! connector with a preferred mode, allocates a dumb buffer, page-flips,
//! and switches /dev/tty0 to KD_GRAPHICS while active to suppress kmsg
//! overlay. Restores the previous tty mode on drop.
