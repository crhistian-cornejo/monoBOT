// build.mjs bundles these as text (esbuild's text loader).
declare module "*.ps1?raw" {
  const text: string;
  export default text;
}
