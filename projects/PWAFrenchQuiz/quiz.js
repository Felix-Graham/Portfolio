// ─────────────────────────────────────────────
// quiz.js — QuizEngine for PWA
// ─────────────────────────────────────────────

const QuizEngine = (() => {
    let vocabList = []   // flat [fr, en, fr, en, …]
    let lastIndex = -1

    // ── Loading ───────────────────────────────

    async function fetchText(url) {
        const res = await fetch(url)
        if (!res.ok) throw new Error(`Failed to fetch ${url}`)
        return res.text()
    }

    function parseVocab(text) {
        const lines = text.split("\n")
            .map(l => l.trim())
            .filter(l => l.length > 0)
        // Skip one header line (matches file format: "Vocab X.X")
        return lines.length > 1 ? lines.slice(1) : lines
    }

    // Load a manifest from vocabDir/index.txt.
    // Each line: filename|label|pairCount   (or bare filename for back-compat)
    async function loadManifest(vocabDir) {
        const text = await fetchText(`${vocabDir}index.txt`)
        return text.split("\n")
            .map(l => l.trim())
            .filter(l => l.length > 0 && !l.startsWith("#"))
            .map(l => {
                const parts = l.split("|")
                return {
                    file:  parts[0].trim(),
                    label: parts[1] ? parts[1].trim() : parts[0].trim(),
                    count: parts[2] ? parseInt(parts[2], 10) : null
                }
            })
            .filter(e => e.file !== "index.txt")
    }

    // Load specific files (array of filenames) from vocabDir.
    async function loadFiles(vocabDir, files) {
        vocabList = []
        lastIndex = -1
        await Promise.all(files.map(async file => {
            try {
                const text = await fetchText(`${vocabDir}${file}`)
                const pairs = parseVocab(text)
                vocabList.push(...pairs)
            } catch (e) {
                console.warn(`Skipping ${file}: ${e.message}`)
            }
        }))
        return Math.floor(vocabList.length / 2)
    }

    // Legacy helper: load everything in the directory (reads all files in manifest)
    async function loadAll(vocabDir) {
        const manifest = await loadManifest(vocabDir)
        return loadFiles(vocabDir, manifest.map(e => e.file))
    }

    // ── Picking ───────────────────────────────

    function nextPair() {
        if (vocabList.length < 2) return null
        let r, attempts = 0
        do {
            r = Math.floor(Math.random() * Math.floor(vocabList.length / 2)) * 2 + 1
            attempts++
        } while (r === lastIndex && attempts < 20)
        lastIndex = r
        return { french: vocabList[r - 1], english: vocabList[r] }
    }

    // Return a question with 4 shuffled choices for multiple-choice mode.
    // { french, correct, choices: [string × 4] }
    function nextChoices() {
        const pair = nextPair()
        if (!pair) return null

        // Pool of all English answers except the correct one
        const pool = []
        for (let i = 1; i < vocabList.length; i += 2) {
            if (vocabList[i] !== pair.english) pool.push(vocabList[i])
        }

        // Pick 3 random distractors (no duplicates)
        const distractors = []
        const seen = new Set()
        let attempts = 0
        while (distractors.length < 3 && pool.length > 0 && attempts < 100) {
            const idx = Math.floor(Math.random() * pool.length)
            const val = pool[idx]
            if (!seen.has(val)) { seen.add(val); distractors.push(val) }
            attempts++
        }

        // Shuffle correct answer in
        const choices = [pair.english, ...distractors]
        for (let i = choices.length - 1; i > 0; i--) {
            const j = Math.floor(Math.random() * (i + 1));
            [choices[i], choices[j]] = [choices[j], choices[i]]
        }

        return { french: pair.french, correct: pair.english, choices }
    }

    // ── Validation ────────────────────────────

    const articles = ["the ", "a ", "an ", "and ", "of ", "for ", "to ", "to be "]

    function strip(s) {
        let r = s.trim().toLowerCase()
        for (const a of articles) {
            if (r.startsWith(a)) r = r.slice(a.length)
            r = r.split(` ${a}`).join(" ")
        }
        return r.trim()
    }

    function jaro(a, b) {
        if (a === b) return 1
        const la = a.length, lb = b.length
        const range = Math.floor(Math.max(la, lb) / 2) - 1
        const am = new Array(la).fill(false), bm = new Array(lb).fill(false)
        let matches = 0
        for (let i = 0; i < la; i++) {
            const s = Math.max(0, i - range), e = Math.min(i + range + 1, lb)
            for (let j = s; j < e; j++) {
                if (bm[j] || a[i] !== b[j]) continue
                am[i] = bm[j] = true; matches++; break
            }
        }
        if (!matches) return 0
        let t = 0, k = 0
        for (let i = 0; i < la; i++) {
            if (!am[i]) continue
            while (!bm[k]) k++
            if (a[i] !== b[k]) t++
            k++
        }
        return (matches / la + matches / lb + (matches - t / 2) / matches) / 3
    }

    function validate(userAns, correct) {
        const u = userAns.trim().toLowerCase()
        const c = correct.trim().toLowerCase()
        if (u === c) return true
        const us = strip(u), cs = strip(c)
        if (us === cs) return true
        return Math.round(jaro(us, cs) * 100) > 80
    }

    return {
        loadManifest,
        loadFiles,
        loadAll,
        nextPair,
        nextChoices,
        validate,
        vocabCount: () => Math.floor(vocabList.length / 2)
    }
})()
