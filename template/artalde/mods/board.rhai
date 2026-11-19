// Board: one column per state, loudest on the left.
// The compact toggle is mod state: it survives edits to this file.

fn render(ctx) {
    let compact = ctx.state.compact ?? false;
    let columns = [];
    for st in ["blocked", "done", "working", "idle"] {
        let items = ctx.sessions.filter(|s| s.state == st || (st == "idle" && s.state == "unknown"));
        let cards = [row([dot().color(st), text(st).bold(), text(`${items.len()}`).color("dim")])];
        for s in items {
            let body = [text(clip(s.name, 60)).bold()];
            if !compact {
                body.push(text(`${s.repo ?? "—"}${if s.branch != () { " · " + s.branch } else { "" }}`).color("dim").size(11));
                let said = s.message ?? s.last_said ?? "";
                // Previews are plain text: drop markdown emphasis and code ticks.
                said.replace("**", "");
                said.replace("`", "");
                if said != "" { body.push(text(clip(said, 160)).size(11)); }
            }
            body.push(row([button("Focus", cmd("session.focus", s.id)), text(ago(s.age)).color("dim").size(11)]));
            if s.resources.len() > 0 {
                body.push(row(s.resources.extract(0..3).map(|r| text(r.id.split("/").pop() ?? r.id).color("dim").size(10).mono())));
            }
            let c = card(body).fill(s.tag ?? st).on_click(cmd("agent.select", s.id));
            cards.push(if s.selected { c.color("accent") } else { c });
        }
        columns.push(col(cards));
    }
    col([
        row([button(if compact { "Expand" } else { "Compact" }, set("compact", !compact))]),
        grid(4, columns),
    ])
}
