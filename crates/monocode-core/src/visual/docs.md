Native session visuals
Use visual_check to validate a visual, then visual_show to publish it into this
conversation. They accept {source}; visual_show also accepts {title?,id?}.
The source is a JavaScript function body with a `visual` SDK. Always return a render function. Use closure state for interactions. Example:
const state = visual.state({count: 0});
return () => visual.column([
  visual.text('Counter', {size: 'title'}),
  visual.stat('Count', String(state.count)),
  visual.button('Add', () => { state.count += 1; })
], {gap: 12});

Answers that tell a story: publish one small visual per idea (a single chart,
a stat row, a gauge) rather than one dashboard holding everything. Each
visual_show returns an embed line such as ![Deploys per week](visual:deploys).
Write your final reply as prose and put each embed line on its own line
directly after the sentence it supports, so text and visuals alternate. The
title becomes the caption; do not restate what the visual already shows.
Visuals you never place appear above your reply. Choose short, readable ids.

Native components:
visual.column(children,{gap?}), row(children,{gap?}), grid(children,{columns?,gap?})
visual.card(children,{gap?}) groups a chart or stats on a raised panel.
visual.text(text,{size?,color?})
visual.stat(label,value,{detail?,delta?,trend?,better?,color?}): delta is a signed
change such as '+12.4%'; trend is an array of numbers drawn as a sparkline;
better:'lower' colors a falling delta as good (latency, errors, cost).
visual.table(columns,rows)
visual.chart('bar'|'line'|'area',[{label,values,color?}],{labels?,height?})
A single-series bar chart labels its latest bar; give each series its own color.
visual.gauge(label,value,{min?,max?,unit?,color?}) for a reading against a range.
visual.donut([{label,value,color?}],{label?,value?}) for parts of a whole;
label and value are written in the middle (default: the total).
visual.heatmap(rows,{rows?,columns?,color?}): rows is an array of equal-length
number arrays, such as weekdays by weeks; labels are optional.
visual.progress(label,value,{max?,detail?,color?})
visual.button(label,onClick,{disabled?}), toggle(label,value,onChange)
visual.slider(label,value,min,max,onChange,{step?}), tabs(labels,selected,onChange)
visual.input(label,value,onChange,{placeholder?}), select(label,options,selectedIndex,onChange)
Input handlers receive a string (up to 4096 bytes); select handlers receive an index.
Select and input labels are not shown: make options and placeholders self-explanatory.
visual.canvas(shapes,{width?,height?})
Canvas shapes: visual.rect(x,y,w,h,{color?}), circle(x,y,r,{color?}),
line(x1,y1,x2,y2,{color?,width?}), polyline([[x,y],...],{color?,width?}),
label(x,y,text,{color?}).
visual.scene(objects,{width?,height?,camera?})
Scene objects: visual.object({kind:'box'|'sphere'|'cylinder'|'cone'|'plane'},
{position:[x,y,z],rotation:[x,y,z],scale:[x,y,z],color:'accent'}).
Custom geometry: {kind:'mesh',vertices:[[x,y,z],...],indices:[[a,b,c],...]}
or {kind:'line',points:[[x,y,z],...],width:1}. Rotations are radians.
Camera: {eye:[5,4,6],target:[0,0,0],fov:45}. Use controls and state to change
geometry or camera; every handler rerenders. State is local to this visual.

Style is inherited from the app. Use semantic colors only: ink, muted, faint,
accent, positive, negative, warning, series1 through series6. Text sizes are
caption, small, body, subtitle, title, hero. Choose clear labels and restrained
headings, use row/column/grid layouts, and allow the host width to determine
layout. Use gaps around 8–16 and chart/canvas/scene heights around 220–360.
Do not draw custom buttons, simulate a whole webpage, add a page background, or
hardcode font families or CSS. Native controls and the app theme own those styles.
Include units, chart legends, readable labels, and useful empty states. Keep
supporting prose in the conversation.
The sandbox has no DOM, network, files, timers, imports, or external libraries.
No HTML, SVG markup, or shader code. Source is limited to 64 KiB; keep
scripts compact. Check validates execution, the initial component tree, and representative controls;
it does not capture a screenshot. Fix errors before publishing. Set a concise
title for the published visual. Never expose capability environment variables.
