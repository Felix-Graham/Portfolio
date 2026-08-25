// ─────────────────────────────────────────────
//   data/about-config.js
//   about page — content & structure
//   Every section on /about.html is generated
//   from this file. To add, remove, or reorder
//   a section, edit the `sections` array —
//   nothing in the root-level about.js needs
//   to change.
// ─────────────────────────────────────────────

export const aboutPage = {

  // Top index strip — one entry per section, in the order you want
  // them to appear both in the nav and on the page.
  index: [
    { id: 'intro',  label: 'Intro' },
    { id: 'stack',  label: 'Stack' },
    { id: 'path',   label: 'Path' },
    { id: 'beyond', label: 'Beyond code' },
  ],

  sections: [
    {
      id:      'intro',
      eyebrow: '01 — Intro',
      heading: 'Felix Graham',
      body:    'I enjoy solving interesting problems with interesting solutions. Currently an A Level Computer Science student based in Brighton, building small tools to sharpen and hone my abilities.',

      // Each side takes an array of images. Leave empty for a
      // placeholder box, or omit `src` on an individual image to
      // show a labelled placeholder instead of a broken <img>.
      images: {
        left:  [],
        right: [],
      },

      dropdown: null,       // no dropdown on this section
      scrollHeight: 150,    // vh — how long this section "sticks" before the next covers it
      glyphs: true,         // falling-glyph background on/off for this section
    },
    {
      id:      'stack',
      eyebrow: '02 — Stack',
      heading: 'What I reach for',
      body:    'Python and Javascript for most projects, with Java, C and Bash making special appearances.',
      images: {
        left:  [{ src: './assets/aboutImages/code-editor.png', alt: 'Code editor' }],
        right: [],
      },
      dropdown: {
        label:   'More Info',
        type:    'text',        // 'text' or 'image'
        font:    'serif',       // renders in the contrasting serif font
        content: 'I like to use Python for basic scripting and modelling projects, with JS reserved for frontend work. I am currently learning Rust for low and systems-level thinking, Node for extended JS, and QML for native UI application development. I have spent a long time carefully curating my NeoVim environment, making it suitable for all that I do.',
      },
      scrollHeight: 150,
      glyphs: false,
    },

    {
      id:      'path',
      eyebrow: '03  — Path',
      heading: 'How I got here',
      body:    'I first began coding with Python in my GCSE Computer Science course, making simple text adventures and calculators. I often spent time after school in the library with access to a computer which I lacked at home. Here I learnt about additional modules for Python such as Tkinter and Numpy. I then started saving up for my own computer through kitchen work and gardening, which I built at home. I decided that my first OS would be Linux based and so, with the relevant research, fired up Linux Mint Cinnamon. From here I taught myself the basics of Bash scripting and spent 6 months ricing my environment.',
      images: { left: [], right: [] },
      dropdown: {
        label: 'Path',
        type:  'timeline',
        font:  'sans-serif',
        limit: 5,
        items: [
          { date: '09-2022', title: 'Learning Python in Computer Science' },
          { date: '07-2024 -- 11-2024', title: 'First PC With Linux Mint' },
          { date: '08-2024', title: 'First Functional Shell Script' },
          { date: '11-2024 -- 05-2025', title: 'Switch to Arch Linux' },
          { date: '02-2025', title: 'Started Using Hyprland (never gone back)' },
          { date: '06-2025', title: 'Switch to Nixos' },
          { date: '08-2025', title: 'Purchased and Assembled First Laptop from Framework for College' },
        ],
      },
      scrollHeight: 160,
      glyphs: false,
    },

    {
      id:      'beyond',
      eyebrow: '04 — Beyond code',
      heading: 'When I\'m not at a keyboard',
      body:    'Filling my brain with anything I can think of, from penetration testing tools to Finnish mythology. I also enjoy taking long walks, listening to music and drinking delicious coffee.',
      images: { left: [], right: [] },
      dropdown: {
        label: 'Recently Researched',
        type:  'timeline',
        font:  'sans-serif',   // matches --sans-serif variant defined in style.css

        // Add as many entries as you like. `date` accepts either:
        //   'MM-YYYY'               — a single point in time
        //   'MM-YYYY -- MM-YYYY'    — a period (handy for career
        //                             history / role durations) —
        //                             e.g. '09-2024 -- 06-2025'
        // Sorting always uses the most recent end of whatever's
        // given, so ranges surface based on when they finished.
        // The dropdown always sorts newest-first and only shows the
        // top `limit` (default 3), so older entries stay recorded
        // here without cluttering the page.
        limit: 3,
        items: [
          { date: '09-2026', title: 'Galactic Algorithms' },
          { date: '09-2026', title: 'Bra and Ket Notation' },
          { date: '08-2026', title: 'The Yongle Emperor' },
          { date: '08-2026', title: 'Cyclic Group' },
          { date: '08-2026', title: 'Howitz Zeta Function' },
          { date: '07-2026', title: 'Qiskit\'s use cases at IBM' },
          { date: '03-2026', title: 'The Kalevala and Finnish mythology' },
          // { date: '09-2025 -- 06-2026', title: 'Example: a role or placement, shown as a period' },
        ],
      },
      scrollHeight: 140,
      glyphs: true,
    },
  ],

};
