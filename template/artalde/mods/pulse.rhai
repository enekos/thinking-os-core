// Pulse: one line of what needs you. Cmd+J jumps to the next blocked or done agent.

fn render(ctx) {
    let items = [];
    for st in ["blocked", "done", "working", "idle"] {
        let n = ctx.counts[st];
        let b = button(`${n} ${st}`, cmd("agent.next")).color(st);
        items.push(if n == 0 { b.color("dim") } else { b });
    }
    let open = ctx.inbox.filter(|i| !i.done);
    let review = open.filter(|i| i.kind == "review").len();
    let red = open.filter(|i| i.kind == "red").len();
    items.push(sep());
    items.push(text(`${review} reviews`).color(if review > 0 { "blocked" } else { "dim" }));
    items.push(text(`${red} red`).color(if red > 0 { "danger" } else { "dim" }));
    items.push(text(`${ctx.prs.len()} open PRs`).color("dim"));
    if ctx.filter != () {
        items.push(sep());
        items.push(button(`× ${ctx.filter}`, cmd("space.filter", "")).color("accent"));
    }
    row(items)
}
