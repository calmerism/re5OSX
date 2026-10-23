// Mods in and out of the game as zips (used from Rust, src/actions.rs).

// Asks for a .zip and sends it to `url` (the game's /import, with the
// token): the imported mod's name, null when none was chosen.
export function importMod(url) {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".zip,application/zip";
    input.onchange = async () => {
      const file = input.files && input.files[0];
      if (!file) return resolve(null);
      try {
        const r = await fetch(url + "&name=" + encodeURIComponent(file.name), { method: "POST", body: file });
        const text = await r.text();
        if (r.ok) resolve(JSON.parse(text).mod); else reject(text);
      } catch (e) {
        reject(String(e));
      }
    };
    input.click();
  });
}
