# Portfolio

A minimal, monochrome portfolio site. Nothing-inspired aesthetic — black and white, typographically precise, deliberate motion.

## File structure

```
/
├── index.html          ← HTML shell (rarely needs editing)
├── style.css           ← All styles and design tokens
├── main.js             ← Renders everything from data files
├── data/
│   ├── config.js       ← Your name, bio, social links ← EDIT THIS
│   └── projects.js     ← Your projects               ← EDIT THIS
└── assets/
    └── (your images)
```

## Getting started

Because the site uses ES modules (`import`/`export`), you need a local dev server — opening `index.html` directly in a browser won't work.

**Option 1 — VS Code (recommended)**
Install the [Live Server](https://marketplace.visualstudio.com/items?itemName=ritwickdey.LiveServer) extension, right-click `index.html` → *Open with Live Server*.

**Option 2 — Node.js**
```bash
npx serve .
```

**Option 3 — Python**
```bash
python3 -m http.server 8080
```

Then open `http://localhost:8080` (or whatever port is shown).

---

## Customising your identity

Edit `data/config.js`:

```js
export const config = {
  name:      'Jane Smith',
  role:      'Creative Developer',
  location:  'London',
  available: true,                    // false hides the status dot
  headline:  ['Building things', 'that <em>think</em>', 'and move.'],
  bio:       'Your bio here...',
  social: {
    github:  'https://github.com/janesmith',
    twitter: 'https://twitter.com/janesmith',
    email:   'jane@example.com',
  },
  year: 2025,
};
```

---

## Adding a project

Open `data/projects.js` and append an object to the array:

```js
{
  title:       'My New Project',
  sub:         'One-line description for the collapsed row',
  tags:        ['Tool', 'Web'],     // max ~3 tags
  year:        '2025',
  image:       './assets/my-project.jpg',  // or null for placeholder
  description: 'Longer description shown when the row is expanded.',
  links: [
    { label: 'Live demo', icon: 'external-link', href: 'https://...' },
    { label: 'Source',    icon: 'github',        href: 'https://...' },
  ],
},
```

**Available link icons:** `external-link` · `github` · `download` · `file-text`

**Index numbers are automatic** — the `01`, `02`... labels are generated from array position, so you never need to renumber when reordering.

---

## Removing a project

Delete its object from the array in `data/projects.js`. Done.

---

## Adding project images

1. Drop your image into the `assets/` folder (JPG or PNG, landscape works best).
2. Set the `image` field: `'./assets/your-image.jpg'`

Images are automatically rendered in greyscale to stay within the monochrome palette. They desaturate slightly on hover.

---

## Deploying

This is a static site — no build step required.

**Netlify / Vercel:** Drag and drop the folder, or connect your GitHub repo. Set publish directory to `/` (root).

**GitHub Pages:** Push to a repo, enable Pages from Settings → Pages → Deploy from branch (`main`, root).
