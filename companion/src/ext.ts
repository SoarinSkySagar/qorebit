/** The browser's extension API: `browser` (Firefox) or `chrome` (Chrome, Brave, Edge); undefined on the website. */
export const ext: any = (globalThis as any).browser ?? (globalThis as any).chrome;
