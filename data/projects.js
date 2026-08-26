// ─────────────────────────────────────────────
//   projects
// ─────────────────────────────────────────────

export const projects = [
  {
    title:       'French Quiz',
    sub:         'Python Based French Vocab Quiz',
    tags:        ['Python', 'Educational'],
    year:        '2025',
    image:       null,
    description: 'A python based tool with multiple different modes for testing on the prescribed vocabulary for AQA A Level French. Hosted on GitHub as a beginner project to help my classmates and myself.',
    links: [
      { label: 'Live demo', icon: 'external-link', href: 'https://felix-graham.github.io/Portfolio/projects/PWAFrenchQuiz/' },
      { label: 'Source',    icon: 'github',        href: 'https://github.com/Felix-Graham/French-Vocab-Quiz.git' },
    ],
  },

  {
    title:       'JS Currency Converter',
    sub:         'Global Currency Converter WebApp Written In JavaScript',
    tags:        ['JavaScript', 'WebApp'],
    year:        '2026',
    image:       'https://felix-graham.github.io/Portfolio/projects/jsCurrencyConverter/demo.png',
    description: 'A JavaScript focused WebApp using APIs to convert currencies. A simple project to demonstrate the use of APIs in JavaScript. I used this as an introductory lesson to teach myself web technologies with scripting.',
    links: [
      { label: 'Live demo', icon: 'external-link', href: 'https://felix-graham.github.io/Portfolio/projects/jsCurrencyConverter/' },
      { label: 'Source',    icon: 'github',        href: 'https://github.com/Felix-Graham/Portfolio/tree/main/projects/jsCurrencyConverter' },
    ],
  },

  {
    title:       'Monte Carlo Stock Simulation',
    sub:         'A rudimentary Monte Carlo Stock Simulation in Python',
    tags:        ['Python', 'Simulation'],
    year:        '2026',
    image:       'https://felix-graham.github.io/Portfolio/projects/stockPredictionModel/demo.png',
    description: 'A Python script using Numpy, Matplotlib and Requests to show 500 possible futures for a stock. A mean average line is then drawn on the graph to show a blend of the results. Whilst this looks very helpful, it is lacking in accuracy due to the stochastic nature of the results.',
    links: [
      
    ],
  },

  {
    title:       'Electron App Music Player',
    sub:         'A Simple Electron App using Mpris and Playerctl to Display the Current Song',
    tags:        ['NodeJS', 'Playerctl'],
    year:        '2026',
    image:       'https://felix-graham.github.io/Portfolio/projects/electronMusicPlayer/demo.png',
    description: 'A very basic NodeJS project to show the current song being played in a music player. It also has Cava integration to pleasantly visualise the music. Whilst I have enjoyed using Electron through NodeJS, I can appreciate its innefficiency. I would like to learn faster methods in the future.',
    links: [
      { label: 'Source',    icon: 'github',        href: 'https://github.com/Felix-Graham/Portfolio/tree/main/projects/electronMusicPlayer' },     
    ],
  },

  {
    title:       'Better mkdir Command',
    sub:         'An Incredibly Simplistic Shell Script To Create Directories and Move Into Them Automatically',
    tags:        ['Bash', 'Nix'],
    year:        '2026',
    image:       null,
    description: 'I made this shell script after realising that almost everytime I created a new directory, I would have to spend precious secounds entering it. The script itself is unbeleivably basic (4 lines excluding comments), but that wasn\'t the real challenge. For it to be functional I had to link it to a real command which was a completely new concept to me. This was made harder by the fact that I main NixOS on my laptop, where I carry out most of my work. To get this to work I had to learn to use my home.nix file to link shell scripts to commands. Once it was done I had learnt far more of how my system works and how to use it.',
    links: [
      { label: 'Source',    icon: 'github',        href: 'https://github.com/Felix-Graham/Portfolio/tree/main/projects/betterMkdir' },     
    ],
  },
];
