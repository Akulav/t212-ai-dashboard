// Aura Portfolio report template (Typst 0.15).
// All data arrives as one JSON string in `sys.inputs.data`, pre-formatted
// by Rust (src/report/mod.rs), so this file is presentation only.

#let d = json(bytes(sys.inputs.data))

#let teal = rgb("#0D9488")
#let violet = rgb("#7C3AED")
#let ink = rgb("#0F172A")
#let muted = rgb("#64748B")
#let hair = rgb("#E2E8F0")
#let soft = rgb("#F8FAFC")
#let pos = rgb("#16A34A")
#let neg = rgb("#DC2626")
#let amber = rgb("#D97706")
#let brand = gradient.linear(teal, violet)

#set document(title: d.title, author: "Akulav")
#set text(font: ("Segoe UI", "Inter", "Helvetica Neue", "Noto Sans", "DejaVu Sans", "Libertinus Serif"), size: 9.5pt, fill: ink)
#set par(leading: 0.62em)
#set page(
  paper: "a4",
  margin: (x: 16mm, top: 16mm, bottom: 20mm),
  footer: context {
    set text(7.5pt, fill: muted)
    line(length: 100%, stroke: 0.5pt + hair)
    v(-4pt)
    grid(
      columns: (1fr, auto),
      [Aura Portfolio · *Powered by AI* · Made by *Akulav*],
      counter(page).display("1 / 1", both: true),
    )
  },
)

#let tone(name) = if name == "pos" { pos } else if name == "neg" { neg } else { ink }
#let signal-color(s) = if s == "Opportunity" { pos } else if s == "Review" { neg } else if s == "Hold" { teal } else { muted }
#let verdict-color(v) = if v == "Add" { pos } else if v == "Trim" { neg } else if v == "Watch" { amber } else { teal }

#let pill(label, col) = box(
  inset: (x: 6pt, y: 2.5pt),
  radius: 7pt,
  fill: col.lighten(86%),
  text(7.5pt, weight: "bold", fill: col)[#label],
)

#let section(title, sub: none) = {
  v(14pt)
  grid(
    columns: (1fr, auto),
    align: bottom,
    text(13pt, weight: "bold")[#title],
    if sub != none { text(8pt, fill: muted)[#sub] },
  )
  v(-5pt)
  line(length: 100%, stroke: 1.4pt + brand)
  v(4pt)
}

#let kpi(k) = box(width: 100%, inset: 10pt, radius: 8pt, stroke: 0.6pt + hair, fill: soft)[
  #text(7pt, weight: "bold", fill: muted, tracking: 0.5pt)[#upper(k.label)]
  #v(-2pt)
  #text(15pt, weight: "bold", fill: tone(k.tone))[#k.value]
  #v(-4pt)
  #text(7.5pt, fill: muted)[#k.caption]
]

#let bullets(items) = list(marker: text(fill: teal)[•], spacing: 5pt, ..items.map(i => [#i]))

// ── Cover band ────────────────────────────────────────────────────────────
#box(width: 100%, radius: 12pt, fill: gradient.linear(rgb("#0F766E"), rgb("#5B21B6")), inset: 18pt)[
  #grid(
    columns: (auto, 1fr, auto),
    gutter: 14pt,
    align: horizon,
    image("logo.png", width: 44pt),
    [
      #text(20pt, weight: "bold", fill: white)[#d.title] \
      #text(9pt, fill: white.transparentize(15%))[#d.subtitle]
    ],
    align(right)[
      #text(7.5pt, fill: white.transparentize(25%), tracking: 0.5pt)[POWERED BY AI] \
      #text(10pt, weight: "bold", fill: white)[Made by Akulav]
    ],
  )
]
#if d.demo [
  #v(6pt)
  #align(center, pill("Demo data — sample portfolio, not a real account", amber))
]

#v(10pt)
#grid(columns: (1fr,) * 4, gutter: 8pt, ..d.kpis.map(kpi))

// ── AI summary ────────────────────────────────────────────────────────────
#section("AI summary", sub: d.model_line)
#if d.ai != none [
  #text(12pt, weight: "bold")[#d.ai.headline]
  #v(2pt)
  #d.ai.overview
  #grid(
    columns: (1fr, 1fr),
    gutter: 14pt,
    [
      #v(6pt)
      #text(9pt, weight: "bold", fill: teal)[RECOMMENDATIONS]
      #bullets(d.ai.recommendations)
    ],
    [
      #v(6pt)
      #text(9pt, weight: "bold", fill: neg)[KEY RISKS]
      #bullets(d.ai.risks)
    ],
  )
] else [
  #text(fill: muted)[#d.ai_note]
]

// ── Opportunities ─────────────────────────────────────────────────────────
#section("Buy opportunities", sub: "valuation + quality screen on your holdings")
#if d.ai != none and d.ai.opportunities.len() > 0 [
  #text(9pt, weight: "bold", fill: pos)[AI VIEW]
  #bullets(d.ai.opportunities)
  #v(4pt)
]
#if d.opportunities.len() > 0 {
  grid(
    columns: (1fr, 1fr),
    gutter: 8pt,
    ..d.opportunities.map(o => box(width: 100%, inset: 9pt, radius: 8pt, stroke: 0.6pt + pos.lighten(60%), fill: pos.lighten(94%))[
      #grid(columns: (1fr, auto), [*#o.ticker* #h(3pt) #text(fill: muted)[#o.name]], pill("Score " + o.score, pos))
      #v(-2pt)
      #text(8pt)[#o.reasons]
    ]),
  )
} else [
  #text(fill: muted)[No holding currently passes the opportunity screen (overall score ≥ 65 and valuation ≥ 55).]
]

#if d.flags.len() > 0 [
  #section("Portfolio observations")
  #bullets(d.flags)
]

// ── Allocation ────────────────────────────────────────────────────────────
#section("Allocation by sector")
#for s in d.sectors [
  #grid(
    columns: (32%, 1fr, 40pt),
    gutter: 8pt,
    align: horizon,
    s.name,
    box(width: 100%, height: 7pt, radius: 3.5pt, fill: hair, box(width: s.weight * 1%, height: 7pt, radius: 3.5pt, fill: brand)),
    align(right, s.label),
  )
  #v(-3pt)
]

// ── Holdings table ────────────────────────────────────────────────────────
#section("Holdings", sub: d.holdings_note)
#table(
  columns: (auto, 1fr, auto, auto, auto, auto, auto, auto),
  stroke: none,
  inset: (x: 5pt, y: 4.5pt),
  fill: (_, y) => if y == 0 { rgb("#F1F5F9") } else if calc.even(y) { soft } else { white },
  align: (x, _) => if x >= 2 and x <= 6 { right } else { left },
  table.header(..("Ticker", "Name", "Weight", "Value", "P/L", "P/E", "Score", "Signal").map(h => text(7pt, weight: "bold", fill: muted)[#upper(h)])),
  ..d.holdings.map(h => (
    text(weight: "bold")[#h.ticker],
    h.name,
    h.weight,
    h.value,
    text(fill: tone(h.pnl_tone))[#h.pnl],
    h.pe,
    h.score,
    pill(h.signal, signal-color(h.signal)),
  )).flatten(),
)

// ── Per-holding analysis ──────────────────────────────────────────────────
#section("Holding analysis", sub: "largest positions first")
#for a in d.analyses {
  block(breakable: false, width: 100%, inset: 11pt, radius: 9pt, stroke: 0.6pt + hair, below: 9pt)[
    #grid(
      columns: (1fr, auto),
      align: horizon,
      [#text(11pt, weight: "bold")[#a.ticker] #h(4pt) #text(fill: muted)[#a.name]],
      [#pill(a.signal, signal-color(a.signal)) #h(3pt) #pill("Score " + a.score, teal)],
    )
    #v(2pt)
    #grid(
      columns: (1fr,) * 4,
      gutter: 6pt,
      ..a.metrics.map(m => box(width: 100%, inset: 6pt, radius: 5pt, fill: soft)[
        #text(6.5pt, weight: "bold", fill: muted)[#upper(m.k)] \
        #text(9.5pt, weight: "bold")[#m.v]
      ]),
    )
    #if a.reasons.len() > 0 [
      #v(3pt)
      #bullets(a.reasons)
    ]
    #if a.ai != none [
      #v(3pt)
      #box(width: 100%, inset: 8pt, radius: 6pt, fill: violet.lighten(93%))[
        #text(7.5pt, weight: "bold", fill: violet)[AI VIEW] #h(4pt) #pill(a.ai.verdict, verdict-color(a.ai.verdict)) \
        #a.ai.thesis
        #if a.ai.strengths.len() > 0 [ \ #text(fill: pos)[*+*] #a.ai.strengths.join(" · ") ]
        #if a.ai.risks.len() > 0 [ \ #text(fill: neg)[*−*] #a.ai.risks.join(" · ") ]
      ]
    ]
  ]
}

// ── Income ────────────────────────────────────────────────────────────────
#section("Dividend income")
#grid(columns: (1fr,) * 4, gutter: 8pt, ..d.income.map(kpi))

// ── Disclaimer ────────────────────────────────────────────────────────────
#v(16pt)
#box(width: 100%, inset: 10pt, radius: 8pt, fill: soft, stroke: 0.6pt + hair)[
  #text(7.5pt, fill: muted)[
    *Not financial advice.* This report was generated by Aura Portfolio from your Trading212 data, #d.provider_line
    and, where indicated, a local AI model running on your own computer. AI-written text can be wrong or
    incomplete; scores are mechanical screens, not recommendations to buy or sell. Do your own research and
    consider professional advice before investing. Generated #d.generated.
  ]
]
