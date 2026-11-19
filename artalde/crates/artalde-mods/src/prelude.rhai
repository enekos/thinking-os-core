// Merged into every mod. Constructors return plain maps; the host turns
// them into widgets. Modifiers are called method-style: text("x").color("done").

fn text(s) { #{ t: "text", text: `${s}` } }
fn heading(s) { #{ t: "heading", text: `${s}` } }
fn row(c) { #{ t: "row", children: c } }
fn col(c) { #{ t: "col", children: c } }
fn wrap(c) { #{ t: "wrap", children: c } }
fn grid(n, c) { #{ t: "grid", cols: n, children: c } }
fn card(c) { #{ t: "card", children: c } }
fn button(label, action) { #{ t: "button", text: `${label}`, action: action } }
fn badge(s) { #{ t: "badge", text: `${s}` } }
fn dot() { #{ t: "dot" } }
fn sep() { #{ t: "sep" } }
fn space(n) { #{ t: "space", value: n } }
fn bar(v) { #{ t: "bar", value: v } }

fn color(c) { this.color = c; this }
fn fill(c) { this.fill = c; this }
fn size(n) { this.size = n; this }
fn bold() { this.bold = true; this }
fn mono() { this.mono = true; this }
fn tip(s) { this.tip = `${s}`; this }
fn on_click(a) { this.action = a; this }

fn cmd(name) { #{ cmd: name } }
fn cmd(name, arg) { #{ cmd: name, arg: `${arg}` } }
fn set(key, value) { let m = #{}; m[key] = value; #{ set: m } }
