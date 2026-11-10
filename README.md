# Allergen Label Printer

A small Windows app that turns a products spreadsheet into one big button per product. Press a button and it prints the product name with its ingredients underneath, allergens in bold, on a Star TSP100III (or any Windows printer).

`AllergenLabels.exe` is a single file with nothing to install. It only uses DLLs that come with Windows. A ready-built copy is in [`dist/`](dist/AllergenLabels.exe), and the sample spreadsheet is in [`examples/`](examples/Products.xlsx).

There's also a **web version** that runs in Docker, for a Home Assistant dashboard or any browser. The Windows app does the printing for it. See [Web app and Home Assistant](#web-app-and-home-assistant).

## The spreadsheet

- The app reads the **first sheet** of an `.xlsx` workbook.
- **Row 1:** one product name per column. Each one becomes a button.
- **Shelf life (optional):** a cell reading "Shelf life: 3 days" in a product's column sets that product's best before date (days after printing). Put it in row 2 to keep things tidy. It's never printed as an ingredient. Products without one use the default from Settings.
- **Rows below:** one ingredient per cell. Blank cells are skipped.
- **Bought-in products:** instead of ingredients, put `Barcode: 5010251703129` in a cell and the shop name in the cell below (e.g. `Morrisons`). See below.
- **Allergens go in brackets.** Write e.g. `Butter (milk)` or `Milk chocolate chips (milk, soya)`. Only the text inside the brackets prints in bold: Butter (**milk**). Bold formatting in the spreadsheet is ignored.
- If you'd rather mark allergens by making text bold in the spreadsheet, switch "Allergens are" in Settings to "Bold text in the spreadsheet".
- To add a product, fill in the next empty column. To remove one, delete its column.

`examples/Products.xlsx` is an example with six products, plus a "Read me" sheet explaining the above.

## Using it

1. Put `AllergenLabels.exe` and `Products.xlsx` in the same folder (that's where it looks the first time), or choose the spreadsheet in **Settings…**.
2. Press a product to print its label. The label also appears in the preview panel on the right.
3. **Copies** sets how many labels each press prints.
4. **Refresh products** re-reads the spreadsheet straight away.

### Automatic refresh

- The app checks every minute whether the spreadsheet has been saved, and reloads it if so.
- It also does a full re-read every 60 minutes (change this in Settings).
- New products get new buttons, and the buttons resize to fit the window. When there are too many to fit at a usable size, a scroll bar appears (mouse wheel works too).
- If the file can't be read (for example, mid-save), the app keeps the last good list and tries again.

## What a label looks like

From top to bottom: the product name, "Ingredients:" with the bracketed allergens in bold, the footer line, the best before date, the business name, address and contact, and the cross-contamination warning.

The default warning reads: "Made in a kitchen that handles a range of ingredients. No peanuts are used, but traces of other allergens may be present due to cross-contamination." The Food Standards Agency recommends that "may contain" style warnings name the specific allergens at risk (e.g. "may contain tree nuts") and are based on a risk assessment, so adjust it in Settings to suit the kitchen.

## Bought-in products (barcode lookup)

For things you buy in and resell, the column only needs the product name, an optional shelf life, the barcode, and the shop:

| Shortbread Fingers |
|---|
| Shelf life: 10 days |
| Barcode: 5010251703129 |
| Morrisons |

You can add one without opening the spreadsheet: press **Add bought-in product…**, type or scan the barcode (Enter looks it up and fills in the name), check the product name, optionally add the shop, pick a shelf life, and press **Add product**. The app adds a new column to the spreadsheet (keeping the old file as `Products (backup).xlsx`), adds the button, and opens the checking screen. If the spreadsheet is open in Excel, close it first so the app can save it.

1. The button shows "(check ingredients)" until someone has checked it. Pressing it opens the checking screen instead of printing.
2. The app looks the barcode up on [Open Food Facts](https://world.openfoodfacts.org) (a free, community-run product database). It fills in the ingredients with allergens marked by \*asterisks\*, plus the pack's "may contain" warning, and shows the product name it found so you can confirm it's the right item.
3. Check every word against the pack, fix anything that's wrong or missing (if nothing is found, type it from the pack), and press **Accept and save**. From then on the button prints straight away, without going online.
4. To look at or re-check a bought-in product later (e.g. after a recipe change), **right-click** its button.

The shop and the manufacturer/brand are never printed on the label. Checked details are kept in `%APPDATA%\AllergenLabels\checked_products.json`, by barcode, so changing a product's barcode means it has to be checked again.

Online information can be wrong, incomplete, or out of date, and the pack is the legal source of ingredient and allergen information, which is why nothing prints until a person has checked it.

## Settings

| Setting | What it does |
|---|---|
| Spreadsheet file | The `.xlsx` to read (a network or OneDrive path is fine) |
| Printer | Which printer to use, or the Windows default |
| Label width (mm) | Used for the on-screen preview (72 mm suits 80 mm paper) |
| Extra side margin (mm) | Added inside the printer's own margins |
| Font, sizes | Product name and ingredient text sizes in points |
| Allergens are | Text in (brackets) (default), or bold text in the spreadsheet |
| Ingredient layout | "Ingredients: a, **b**, c." like a UK food label, or one per line |
| Footer text | Optional line under the ingredients, e.g. "Allergens are shown in bold." Leave blank for none |
| Business name, Address, Contact | Printed centred near the bottom of every label (PoFfles, the Mansfield address, and amy@poffles.co.uk by default). Clear any to leave it off |
| Default best before | 1–10 days after printing (default 3), used for products with no shelf life cell in the spreadsheet |
| Warning | Cross-contamination sentence printed at the very bottom |
| Print test label | Prints the first product using the settings on screen, before you save |

Settings are saved in `%APPDATA%\AllergenLabels\settings.json`.

## Star TSP100III setup

The TSP100III only prints graphics, so the app prints through the normal Windows driver, not raw printer commands.

1. Install Star's TSP100 driver (futurePRNT / Star Windows driver) so the printer appears in Windows.
2. In the printer's **Printing Preferences**, choose a receipt-length paper size (e.g. "72mm x Receipt") so each label is only as long as its text, and set the cutter to cut at the end of the document.
3. In the app's Settings, pick the printer and press **Print test label**.

A label too long for the page size set in the driver carries on onto a second page rather than being cut off.

## Web app and Home Assistant

The web app is the same label app as a web page: product buttons, label preview, copies, refresh, adding and checking bought-in products, and the label settings. It reads and writes the same kind of `Products.xlsx`.

On the web page, products are always shown A to Z, and the buttons shrink to fit the screen before any scrolling is needed. The small bin at the top right of a button deletes that product: after you confirm, its column is removed from the spreadsheet (the previous version is kept as `Products (backup).xlsx`).

The Star TSP100III is plugged into a Windows PC by USB, so the web app can't print to it directly. Instead, the Windows app on that PC runs as a **print helper**. It collects labels printed from the web app and prints them, and it still works on its own as before.

```
Home Assistant / browser ──► web app (Docker, 192.168.1.128:8443) ◄── Windows app on the PC ──USB──► TSP100III
                              Products.xlsx, settings, checks          collects and prints labels
```

### 1. Run the web app

On the machine that runs Blockplan (192.168.1.128):

```
git clone https://github.com/Tonyscodehub/ingredients_print
cd ingredients_print
cp .env.example .env
docker compose up -d --build
```

It runs at **https://192.168.1.128:8443**. Blockplan already uses port 443, so this uses 8443; change it with `LABELS_PORT` in `.env`.

- **The spreadsheet** lives in the `data` folder next to `docker-compose.yml`, as `Products.xlsx`. The first time, the sample spreadsheet is put there. Copy your own over it. Settings and checked bought-in products are kept in the same folder.
- **To edit the spreadsheet in Excel,** the `data` folder needs to be reachable from Windows. Share it (e.g. with Samba), or keep the spreadsheet on the NAS and mount that share into the container in `docker-compose.yml` in place of `./data`. Saved changes are picked up within a minute. Close Excel before adding a bought-in product from the web app, or Excel's next save will overwrite the new column.
- The web app needs internet access for barcode lookups.

### 2. HTTPS

HTTPS works the same way as Blockplan's. If there's no certificate in the `certs` folder, the web app makes a self-signed one there covering `localhost`, the machine's name, and the addresses in `HTTPS_HOSTNAMES` (192.168.1.128 by default). To use a real certificate, put `cert.pem` and `key.pem` (or `fullchain.pem` and `privkey.pem`, or a `.crt` and `.key` with the same name) in `certs` and restart. `HTTPS=off` in `.env` switches to plain HTTP.

**For Home Assistant this matters:** a page inside a dashboard can't show the browser's "continue anyway" warning, so with a self-signed certificate the label app is blank until each device trusts the certificate. Any one of these fixes it:

- **Reuse Blockplan's certificate.** If your devices already trust it, and it covers 192.168.1.128, mount Blockplan's `certs` folder instead (see the comment in `docker-compose.yml`).
- **Trust the self-signed certificate** on each computer, tablet, and phone that shows the dashboard, the same way as for Blockplan (install `certs/self-signed-cert.pem` as a trusted root).
- **Use plain HTTP**, but only if you open Home Assistant itself over plain HTTP (e.g. `http://192.168.1.77:8123`). An HTTPS dashboard can't show an HTTP page.

### 3. Set up the print helper

On the PC with the printer, open the Windows app, then **Settings…**:

1. **Web app server:** `https://192.168.1.128:8443`
2. Tick **Accept a self-signed certificate** if the web app uses one.
3. Choose the TSP100III under **Printer**, and **Save**.

The title bar shows "print helper connected to 192.168.1.128", and the web app shows **Printer ready**. Leave the app running (put a shortcut in the Startup folder, set to Run: Minimised, so it starts with Windows). Labels printed while the helper is offline wait up to 10 minutes, then are dropped so nothing prints unexpectedly later.

Labels from the web app use the web app's label settings (business details, warning, sizes, and so on). The printer is always the one chosen on the PC. To stop anyone else on the network collecting print jobs, set `PRINT_HELPER_KEY` in `.env` and put the same key in the Windows app's Settings.

The web app keeps its own list of checked bought-in products, separate from the Windows app's own list, so a product checked in one isn't automatically checked in the other.

### HTTPS through OPNsense (no certificate warnings)

Here the label app is reached at **https://labels.johncock.co.uk**, through HAProxy on OPNsense using the existing `*.johncock.co.uk` Let's Encrypt certificate, so no device needs to trust anything:

- Unbound host override: `labels.johncock.co.uk` → 192.168.1.1 (HAProxy)
- HAProxy: server `labels-web` (192.168.1.128:8443, SSL, no verify), backend `be-labels`, condition `host-labels`, and rule `rule-labels` (LAN clients only) on the HTTPS frontend

### 4. The Home Assistant dashboard

[`homeassistant/dashboard.yaml`](homeassistant/dashboard.yaml) is a full-screen dashboard with the Remootio gate bar across the top and the label app filling the rest (sized for a 1920 × 1080 screen). [`homeassistant/security-cameras.yaml`](homeassistant/security-cameras.yaml) shows the two gate cameras side by side:

1. In Home Assistant: **Settings → Dashboards → Add dashboard → New dashboard from scratch**, then open it.
2. **⋮ → Edit dashboard → ⋮ → Raw configuration editor**, and paste the file in.
3. Check the Remootio entity (its `cover.` entity, from **Settings → Devices & services → Remootio**) and the URL match yours. Save.

Pressing the gate bar asks for confirmation first. To make the label app taller or shorter, change `aspect_ratio`. Inside the dashboard the label app uses a compact layout (`?embed=1`).

## Building from source

Rust (stable) with the `x86_64-pc-windows-gnu` target and MinGW-w64 (or build natively on Windows with `cargo build --release`):

```
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

The web app (Linux) builds with `cargo build --release --features server --bin labels-server`, or with the Dockerfile.

The spreadsheet reader has tests (`cargo test`) that run on any OS.
