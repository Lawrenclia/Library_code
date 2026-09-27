"""Shared presentation layer, inspired by Dmaid's warm calendar palette.

The native ttk controls retain their bindings, focus, state and accessibility.
Nine-slice images only replace the old raised borders; no UI dependency is added.
"""
import math
import tkinter as tk
from dataclasses import dataclass
from tkinter import ttk


@dataclass(frozen=True)
class Palette:
    canvas: str = "#F2F1EF"
    surface: str = "#FCFBF9"
    white: str = "#FFFFFF"
    inset: str = "#EAE8E5"
    line: str = "#D9D5D1"
    ink: str = "#262527"
    muted: str = "#716B6C"
    faint: str = "#AAA3A2"
    accent: str = "#982D3D"
    accent_hover: str = "#AB3548"
    accent_pressed: str = "#792131"
    accent_soft: str = "#F2E3E5"
    amber: str = "#886119"
    amber_soft: str = "#FBF3DE"
    green: str = "#24714F"
    green_soft: str = "#E7F2E9"
    red: str = "#9C3544"
    red_soft: str = "#F8E7E9"


P = Palette()
FONT = "Microsoft YaHei UI"


def _rounded_image(root, fill, outline, radius=8):
    """A small antialiased, stretchable border rendered by Tk itself."""
    size = radius * 2 + 5
    image = tk.PhotoImage(master=root, width=size, height=size)
    colors = [tuple(int(color[i:i + 2], 16) for i in (1, 3, 5))
              for color in (P.canvas, outline, fill)]

    def inside(x, y, inset):
        r = radius - inset
        cx = min(max(x, radius), size - radius)
        cy = min(max(y, radius), size - radius)
        return (inset <= x <= size - inset and inset <= y <= size - inset
                and math.hypot(x - cx, y - cy) <= r)

    rows = []
    for y in range(size):
        row = []
        for x in range(size):
            channels = [0, 0, 0]
            for sy in (.125, .375, .625, .875):
                for sx in (.125, .375, .625, .875):
                    index = 2 if inside(x + sx, y + sy, 1) else 1 if inside(x + sx, y + sy, 0) else 0
                    for channel, value in enumerate(colors[index]):
                        channels[channel] += value
            row.append("#%02x%02x%02x" % tuple(round(value / 16) for value in channels))
        rows.append("{" + " ".join(row) + "}")
    image.put(" ".join(rows))
    return image


def install_theme(root):
    """Install once per Tk interpreter so embedded pages cannot restyle each other."""
    if getattr(root, "_library_theme_images", None):
        return ttk.Style(root)
    style = ttk.Style(root)
    style.theme_use("clam")
    root.configure(background=P.canvas)
    root.option_add("*Font", (FONT, 9))
    root.option_add("*Listbox.background", P.surface)
    root.option_add("*Listbox.foreground", P.ink)
    root.option_add("*Listbox.selectBackground", P.accent_soft)
    root.option_add("*Listbox.selectForeground", P.accent)
    root.option_add("*Toplevel.background", P.canvas)
    images = root._library_theme_images = []

    def element(name, normal, states=()):
        specs = []
        for state, fill, border in states:
            item = _rounded_image(root, fill, border)
            images.append(item)
            specs.append((state, item))
        base = _rounded_image(root, *normal)
        images.append(base)
        style.element_create(name, "image", base, *specs, border=9, sticky="nsew")

    def button(name, fill, border, hover, pressed, foreground):
        edge = "Library." + name + ".border"
        element(edge, (fill, border), [
            ("disabled", P.inset, P.inset),
            ("pressed", pressed, pressed),
            ("active", hover, hover),
            ("focus", fill, P.ink),
        ])
        style.layout(name, [(edge, {"sticky": "nsew", "children": [
            ("Button.padding", {"sticky": "nsew", "children": [
                ("Button.label", {"sticky": "nsew"})]})]})])
        style.configure(name, foreground=foreground, background=fill, borderwidth=0,
                        padding=(7, 5), font=(FONT, 9), anchor="center")
        style.map(name, foreground=[("disabled", P.faint), ("!disabled", foreground)])

    style.configure(".", font=(FONT, 9), background=P.canvas, foreground=P.ink,
                    bordercolor=P.line, lightcolor=P.line, darkcolor=P.line,
                    selectbackground=P.accent_soft, selectforeground=P.accent,
                    troughcolor=P.inset)
    style.configure("TFrame", background=P.canvas)
    style.configure("TLabel", background=P.canvas, foreground=P.ink)
    style.configure("Muted.TLabel", foreground=P.muted)
    style.configure("Eyebrow.TLabel", foreground=P.accent, font=(FONT, 8, "bold"))
    style.configure("Title.TLabel", font=(FONT, 18, "bold"))
    style.configure("Section.TLabel", font=(FONT, 10, "bold"))
    style.configure("Card.TLabel", background=P.surface, foreground=P.muted)
    style.configure("Metric.TLabel", background=P.surface, foreground=P.ink,
                    font=(FONT, 22, "bold"))
    element("Library.card", (P.surface, P.line))
    style.layout("Card.TFrame", [("Library.card", {"sticky": "nsew"})])
    style.configure("Card.TFrame", background=P.surface)

    button("TButton", P.surface, P.line, P.inset, P.line, P.ink)
    for name in ("Primary.TButton", "Model.TButton"):
        button(name, P.accent, P.accent, P.accent_hover, P.accent_pressed, P.white)
    button("Complete.TButton", P.green, P.green, "#2D815C", "#1C5D40", P.white)
    style.configure("Complete.TButton", padding=(12, 8), font=(FONT, 10, "bold"))

    element("Library.tab", (P.inset, P.inset), [
        ("selected", P.surface, P.line), ("active", P.surface, P.surface)])
    style.layout("TNotebook.Tab", [("Library.tab", {"sticky": "nsew", "children": [
        ("Notebook.padding", {"sticky": "nsew", "children": [
            ("Notebook.label", {"sticky": "nsew"})]})]})])
    style.configure("TNotebook", background=P.canvas, borderwidth=0,
                    tabmargins=(0, 0, 0, 10))
    style.configure("TNotebook.Tab", padding=(14, 7), font=(FONT, 9), foreground=P.muted)
    style.map("TNotebook.Tab", foreground=[("selected", P.accent), ("active", P.ink)])

    for name in ("TEntry", "TCombobox", "TSpinbox"):
        style.configure(name, padding=(7, 5), fieldbackground=P.surface,
                        foreground=P.ink, arrowcolor=P.muted, borderwidth=1)
        style.map(name, fieldbackground=[("disabled", P.inset), ("readonly", P.surface)],
                  foreground=[("disabled", P.faint)], bordercolor=[("focus", P.accent)],
                  lightcolor=[("focus", P.accent)], darkcolor=[("focus", P.accent)],
                  selectbackground=[("!disabled", P.accent_soft)],
                  selectforeground=[("!disabled", P.accent)])
    style.configure("TCheckbutton", padding=(0, 2), background=P.canvas)
    style.map("TCheckbutton", background=[("active", P.canvas)],
              foreground=[("disabled", P.faint)])
    style.configure("Treeview", background=P.surface, fieldbackground=P.surface,
                    foreground=P.ink, borderwidth=0, rowheight=30, font=(FONT, 9))
    style.configure("Treeview.Heading", background=P.inset, foreground=P.muted,
                    borderwidth=0, relief="flat", padding=(10, 8), font=(FONT, 9))
    style.map("Treeview.Heading", background=[("active", P.inset)])
    style.map("Treeview", background=[("selected", P.accent_soft)],
              foreground=[("selected", P.accent)])
    for orientation in ("Vertical", "Horizontal"):
        style.configure(orientation + ".TScrollbar", background=P.line,
                        troughcolor=P.canvas, borderwidth=0, arrowsize=11,
                        lightcolor=P.canvas, darkcolor=P.canvas, arrowcolor=P.muted)
    style.configure("Horizontal.TProgressbar", background=P.accent,
                    troughcolor=P.inset, borderwidth=0, thickness=5)
    style.configure("TSeparator", background=P.line)
    return style


def style_text(widget, *, inset=False):
    """Use the same readable surface for all native Text fields."""
    widget.configure(background=P.white if inset else P.surface, foreground=P.ink,
                     insertbackground=P.accent, selectbackground=P.accent_soft,
                     selectforeground=P.ink, relief="flat", borderwidth=0,
                     highlightthickness=1, highlightbackground=P.line,
                     highlightcolor=P.accent, padx=10, pady=7, spacing1=2, spacing3=2)
