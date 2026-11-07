---
name: writing
description: Write documents and replies people read as a careful professional wrote them, then clean their text of invisible characters. Use for every report, document, spreadsheet text, slide or long reply you write for the user, and before you hand a file to anyone.
---

# Writing

Text for people should read as a careful professional wrote it: plain, specific, in the user's language, and clean
of characters nobody can see. This is about quality, not about hiding who wrote it: never alter text to fool a
plagiarism or AI detector, never strip a document's provenance metadata or watermark, and never rewrite someone
else's work to pass it off as new.

## 1. Write plainly

- Short, specific sentences. Numbers with their units, dates with the year, names spelled out the first time.
- Say what a figure means for the decision or the design, not that it is "key" or "crucial".
- A table or a short list over a long paragraph; one idea per paragraph.
- Correct accents and punctuation for the language. In Spanish: opening ¿ and ¡, decimal comma when the user uses it,
  no Title Case in headings.
- No filler that marks text as machine-made. Rewrite it when you see it:
  - Spanish: "cabe destacar", "cabe señalar", "es importante señalar", "en resumen", "en conclusión", "profundizar en",
    "sin lugar a dudas", "juega un papel crucial", "en el panorama actual".
  - English: "it is worth noting", "in summary", "in conclusion", "delve into", "plays a crucial role",
    "a testament to", "navigate the complexities".
- Few dashes: a comma, a colon or a new sentence usually reads better. No decorative emoji.
- Do not open with a restatement of the question or close with an offer to help more.

## 2. Structure a technical document

In this order, leaving out a section only when it has nothing:

1. Resumen (or Summary): the question and the answer in five lines.
2. Introducción: why it matters and the scope.
3. Marco teórico: concepts, norms and criteria.
4. Metodología: sources and how the analysis or design is done.
5. Cálculos: formulas in LaTeX between `$$`, each symbol with its unit, and a worked example.
6. Resultados: tables for criteria and values.
7. Conclusiones y recomendaciones.
8. Referencias: numbered, in the style the user asked for (APA by default).
9. Anexos: the files that go with it (spreadsheets, PDFs, drawings), each with what it holds.

## 3. Clean the text

MonoBOT already cleans documents saved with `artifacts.write` and text the user copies from the chat. A file you write
yourself (Markdown, text, Word, Excel, PowerPoint) is yours to clean, every time, before you say it is ready:

```bash
python <skill>/scripts/clean_text.py <file> [<file> ...]
```

It removes zero-width marks, hidden tag characters, direction overrides, soft hyphens and a stray BOM, makes odd
spaces plain, and in Markdown and text trims trailing spaces and extra blank lines (code fences stay as they are). In
Office files only the invisible characters go; styles, formulas and non-breaking spaces are kept. Clean only files you
created; for someone else's file, write the clean copy beside it with `--out <new file>`.

Before you deliver, check the wording:

```bash
python <skill>/scripts/clean_text.py <file> --check
```

It changes nothing and lists the filler phrases it finds and how many dashes there are per 1000 words. Rewrite what
it flags yourself; there is no automatic rewording.
