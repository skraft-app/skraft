# dmgbuild's settings for Skraft's macOS disk image (scripts/package-macos.sh passes the
# paths with -D): the app and a link to /Applications side by side on the branded background,
# whose arrow lies between their places, and the app's icon on the volume. The background
# (dmg-background.png, its @2x beside it for Retina) is one picture whatever the system's
# appearance: Finder writes the names in white in dark mode, less readable on it.

app = defines["app"]  # noqa: F821 (dmgbuild's namespace)

format = "UDZO"
filesystem = "HFS+"
files = [app]
symlinks = {"Applications": "/Applications"}
hide_extensions = ["Skraft.app"]
icon = defines["icon"]  # noqa: F821
background = defines["background"]  # noqa: F821

# The window: the background's size, in points; the icons centred on its arrow's ends.
window_rect = ((200, 120), (640, 400))
default_view = "icon-view"
show_toolbar = False
show_pathbar = False
show_status_bar = False
show_tab_view = False
show_sidebar = False
icon_size = 128
text_size = 13
icon_locations = {"Skraft.app": (170, 200), "Applications": (470, 200)}
