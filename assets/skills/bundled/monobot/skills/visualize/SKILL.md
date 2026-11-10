---
name: visualize
description: Show a chart, stat row, gauge, diagram or small interactive explainer inline in this monoBOT chat. Use when an answer has data with a trend, comparison or breakdown, or when the user asks to see, plot, graph, chart or visualize something ("grafícalo", "muéstrame un gráfico", "visualize this").
---
# Inline visuals

monoBOT draws visuals in the chat itself, with its own dithered charts and controls. Never start a server, open a
browser, or write an HTML, SVG or image file to show one. Ordinary coding answers need no visual.

1. Load the `monocode_visual` tools if they are deferred (ToolSearch `select:mcp__monocode_visual__visual_docs,mcp__monocode_visual__visual_check,mcp__monocode_visual__visual_show`).
2. Read `visual_docs` once per conversation. It is the whole SDK: a JavaScript function body that returns a render
   function built from `visual.*` components, with semantic colours only.
3. Run `visual_check` with the source and fix every error it reports.
4. Publish with `visual_show` and a short title. Prefer one small visual per idea (a single chart, a stat row) over
   one dashboard holding everything.
5. Put the embed line it returns, such as `![Weekly deploys](visual:deploys)`, on its own line in your reply,
   right after the sentence it supports. Don't restate what the visual shows.

If the tools are missing, the executable in monoBOT's app CLI works too: `monocode visual docs`, then
`monocode visual check --input FILE` and `monocode visual show --input FILE --title TITLE`. If publication fails,
say so; never claim a visual was shown.
