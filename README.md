# Allergen Label Printer

A small Windows app that turns a products spreadsheet into one big button per product. Press a button and it prints the product name with its ingredients underneath, allergens in bold, on a Star TSP100III (or any Windows printer).

`AllergenLabels.exe` is a single file with nothing to install. It only uses DLLs that come with Windows.

## The spreadsheet

- The app reads the **first sheet** of an `.xlsx` workbook.
- **Row 1:** one product name per column. Each one becomes a button.
- **Rows below:** one ingredient per cell. Blank cells are skipped.
- **Bold = allergen.** Bold a whole cell, or just a word inside a cell (e.g. only *soya* in "emulsifier: soya lecithin"). Either way, it prints bold.
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

## Settings

| Setting | What it does |
|---|---|
| Spreadsheet file | The `.xlsx` to read (a network or OneDrive path is fine) |
| Printer | Which printer to use, or the Windows default |
| Label width (mm) | Used for the on-screen preview (72 mm suits 80 mm paper) |
| Extra side margin (mm) | Added inside the printer's own margins |
| Font, sizes | Product name and ingredient text sizes in points |
| Ingredient layout | "Ingredients: a, **b**, c." like a UK food label, or one per line |
| Footer text | Optional line at the bottom, e.g. "Allergens are shown in bold." Leave blank for none |
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
