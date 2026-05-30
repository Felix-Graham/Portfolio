// ─────────────────────────────────────────────
// quiz.js — QuizEngine for PWA
// ─────────────────────────────────────────────

const QuizEngine = (() => {
    let vocabList = []
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
        // Skip one header line (matches your file format)
        return lines.length > 1 ? lines.slice(1) : lines
    }

    async function loadAll(vocabDir) {
        vocabList = []
        const indexText = await fetchText(`${vocabDir}index.txt`)
        const files = indexText.split("\n")
            .map(l => l.trim())
            .filter(l => l.length > 0 && l !== "index.txt")

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

    // ── Picking ───────────────────────────────

    function nextPair() {
        if (vocabList.length < 2) return null
        let r, attempts = 0
        do {
            r = Math.floor(Math.random() * vocabList.length)
            if (r % 2 === 0) r = Math.min(r + 1, vocabList.length - 1)
            attempts++
        } while (r === lastIndex && attempts < 20)
        lastIndex = r
        return { french: vocabList[r - 1], english: vocabList[r] }
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

    return { loadAll, nextPair, validate, vocabCount: () => Math.floor(vocabList.length / 2) }
})()
