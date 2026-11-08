# Allergen Label Printer

A small Windows app that turns a products spreadsheet into one big button per product. Press a button and it prints the product name with its ingredients underneath, allergens in bold, on a Star TSP100III (or any Windows printer).

`AllergenLabels.exe` is a single file with nothing to install. It only uses DLLs that come with Windows. A ready-built copy is in [`dist/`](dist/AllergenLabels.exe), and the sample spreadsheet is in [`examples/`](examples/Products.xlsx).

## The spreadsheet

- The app reads the **first sheet** of an `.xlsx` workbook.
- **Row 1:** one product name per column. Each one becomes a button.
- **Shelf life (optional):** a cell reading "Shelf life: 3 days" in a product's column sets that product's best before date (days after printing). Put it in row 2 to keep things tidy. It's never printed as an ingredient. Products without one use the default from Settings.
- **Rows below:** one ingredient per cell. Blank cells are skipped.
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

## Building from source

Rust (stable) with the `x86_64-pc-windows-gnu` target and MinGW-w64 (or build natively on Windows with `cargo build --release`):

```
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

The spreadsheet reader has tests (`cargo test`) that run on any OS.
