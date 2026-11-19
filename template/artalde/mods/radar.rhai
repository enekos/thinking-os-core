// Radar: every agent as a tile, grouped by the repo it works in.
// Tagged sessions take their tag colour. Click a tile to select it.

fn tile(s) {
    let body = [
        row([dot().color(s.state), text(s.state).color(s.state).size(11), space(4), text(ago(s.age)).color("dim").size(11)]),
        text(clip(s.name, 46)).bold(),
    ];
    if s.handle != () { body.push(text(s.handle).color("dim").size(10).mono()); }
    let line = s.message ?? s.activity ?? s.branch ?? "";
    if line != "" { body.push(text(clip(line, 52)).color("dim").size(11)); }
    if s.resources.len() > 0 {
        let chips = [];
        for r in s.resources {
            if chips.len() < 3 { chips.push(button(r.id.split("/").pop() ?? r.id, cmd("url.open", r.url)).size(10).tip(r.title ?? r.id)); }
        }
        body.push(row(chips));
    }
    let c = card(body).fill(s.tag ?? s.state).on_click(cmd("agent.select", s.id));
    if s.selected { c.color("accent") } else { c }
}

fn render(ctx) {
    let groups = #{};
    for s in ctx.sessions {
        let key = s.repo ?? "elsewhere";
        if !(key in groups) { groups[key] = []; }
        groups[key].push(s);
    }
    let out = [row([
        badge(`${ctx.counts.blocked} blocked`).color("blocked"),
        badge(`${ctx.counts.done} done`).color("done"),
        badge(`${ctx.counts.working} working`).color("working"),
        badge(`${ctx.counts.idle + ctx.counts.unknown} quiet`).color("idle"),
    ])];
    if ctx.sessions.len() == 0 {
        out.push(text("No agents running. Cmd+N starts one.").color("dim"));
    }
    for key in groups.keys() {
        out.push(space(6));
        out.push(row([text(key).bold(), text(`${groups[key].len()}`).color("dim")]));
        out.push(wrap(groups[key].map(|s| tile(s))));
    }
    col(out)
}
