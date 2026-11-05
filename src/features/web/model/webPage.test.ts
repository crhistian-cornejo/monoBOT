// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { readWebPage } from "./webPage";

const ARTICLE = `<!doctype html>
<html lang="es">
<head>
  <title>Pozas de sedimentación | Minería Hoy</title>
  <meta property="og:title" content="Cómo diseñar pozas de sedimentación">
  <meta property="og:description" content="Guía práctica para manejar aguas superficiales en minería.">
  <meta property="og:site_name" content="Minería Hoy">
  <meta property="og:image" content="/img/poza.jpg">
  <meta name="author" content="Ana Pérez">
  <meta property="article:published_time" content="2024-05-02T10:00:00Z">
  <link rel="shortcut icon" href="/static/icon.png">
  <link rel="canonical" href="https://mineria.example/pozas">
  <script type="application/ld+json">{"@graph":[{"@type":"NewsArticle","headline":"Pozas","dateModified":"2024-06-01","publisher":{"name":"Minería Hoy SAC"}}]}</script>
  <script>window.track = "should never appear";</script>
  <style>body { color: red }</style>
</head>
<body>
  <nav><a href="/">Inicio</a> <a href="/contacto">Contacto</a></nav>
  <article>
    <h1>Cómo diseñar pozas de sedimentación</h1>
    <p>Una poza de sedimentación retiene los sólidos del agua de escorrentía antes de descargarla.
       El <a href="https://example.org/guia.pdf">manual de diseño</a> explica el tiempo de retención.</p>
    <h2>Dimensionamiento</h2>
    <ul><li>Caudal de diseño</li><li>Tiempo de retención de 24 horas</li></ul>
    <table><tr><th>Partícula</th><th>Velocidad</th></tr><tr><td>Arena fina</td><td>0.8 cm/s</td></tr></table>
    <p>El área superficial depende de la velocidad de sedimentación de la partícula más fina.</p>
  </article>
  <footer>© 2024 Minería Hoy</footer>
</body>
</html>`;

describe("readWebPage", () => {
  it("reads who published the page, when, and its picture and icon", () => {
    const page = readWebPage(ARTICLE, "https://mineria.example/pozas?utm=1");
    expect(page).toMatchObject({
      title: "Cómo diseñar pozas de sedimentación",
      description: "Guía práctica para manejar aguas superficiales en minería.",
      siteName: "Minería Hoy",
      author: "Ana Pérez",
      published: "2024-05-02T10:00:00Z",
      modified: "2024-06-01",
      language: "es",
      image: "https://mineria.example/img/poza.jpg",
      icon: "https://mineria.example/static/icon.png",
      canonical: "https://mineria.example/pozas",
    });
  });

  it("keeps the article's text as Markdown, without scripts or navigation", () => {
    const page = readWebPage(ARTICLE, "https://mineria.example/pozas");
    expect(page.text).toContain("# Cómo diseñar pozas de sedimentación");
    expect(page.text).toContain("## Dimensionamiento");
    expect(page.text).toContain("- Tiempo de retención de 24 horas");
    expect(page.text).toContain("Partícula | Velocidad\nArena fina | 0.8 cm/s");
    expect(page.text).not.toMatch(/should never appear|color: red|Contacto|© 2024/);
    expect(page.outline).toEqual([
      "Cómo diseñar pozas de sedimentación",
      "Dimensionamiento",
    ]);
    expect(page.links).toEqual([
      { text: "manual de diseño", url: "https://example.org/guia.pdf" },
    ]);
  });

  it("cuts long text at a paragraph and finds the passages asked for", () => {
    const page = readWebPage(ARTICLE, "https://mineria.example/pozas", {
      maxChars: 200,
      query: "velocidad de sedimentación",
    });
    expect(page.truncated).toBe(true);
    expect(page.text.length).toBeLessThanOrEqual(200);
    expect(page.chars).toBeGreaterThan(200);
    expect(page.passages?.[0]).toMatch(/velocidad de sedimentación/);
  });

  it("finds a scholarly page's DOI and PDF", () => {
    const page = readWebPage(
      `<html><head>
        <meta name="citation_title" content="Settling pond design">
        <meta name="citation_author" content="Lee, J."><meta name="citation_author" content="Kim, S.">
        <meta name="citation_doi" content="doi:10.1016/j.mineng.2020.106123">
        <meta name="citation_pdf_url" content="/content/pond.pdf">
        <meta name="citation_journal_title" content="Minerals Engineering">
      </head><body><p>Abstract.</p></body></html>`,
      "https://journal.example/article/1",
    );
    expect(page).toMatchObject({
      title: "Settling pond design",
      author: "Lee, J., Kim, S.",
      siteName: "Minerals Engineering",
      doi: "10.1016/j.mineng.2020.106123",
      pdfUrl: "https://journal.example/content/pond.pdf",
      icon: "https://journal.example/favicon.ico",
    });
  });
});
