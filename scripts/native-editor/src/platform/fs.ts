// Path helper the editor sources import. The native interpreter has no
// filesystem API, so only the pure basename the editor needs lives here.
export const basename = (path: string) => path.replace(/\\/g, "/").replace(/\/+$/, "").split("/").pop() || "/";
