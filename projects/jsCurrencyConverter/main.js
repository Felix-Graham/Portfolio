/* ─────────────────────────────────────────────
   CURRENCY CONVERTER — MAIN JS
   ───────────────────────────────────────────── */

const API_KEY = "11caec3e45d0469b41c96558";

const CURRENCIES = [
  ["USD", "US Dollar"],
  ["EUR", "Euro"],
  ["GBP", "British Pound"],
  ["JPY", "Japanese Yen"],
  ["CHF", "Swiss Franc"],
  ["CAD", "Canadian Dollar"],
  ["AUD", "Australian Dollar"],
  ["NZD", "New Zealand Dollar"],
  ["CNY", "Chinese Yuan"],
  ["HKD", "Hong Kong Dollar"],
  ["SGD", "Singapore Dollar"],
  ["INR", "Indian Rupee"],
  ["BRL", "Brazilian Real"],
  ["MXN", "Mexican Peso"],
  ["KRW", "South Korean Won"],
  ["SEK", "Swedish Krona"],
  ["NOK", "Norwegian Krone"],
  ["DKK", "Danish Krone"],
  ["PLN", "Polish Zloty"],
  ["CZK", "Czech Koruna"],
  ["HUF", "Hungarian Forint"],
  ["TRY", "Turkish Lira"],
  ["ZAR", "South African Rand"],
  ["SAR", "Saudi Riyal"],
  ["AED", "UAE Dirham"],
  ["THB", "Thai Baht"],
  ["MYR", "Malaysian Ringgit"],
  ["IDR", "Indonesian Rupiah"],
  ["PHP", "Philippine Peso"],
  ["VND", "Vietnamese Dong"]
];

document.addEventListener("DOMContentLoaded", function () {

  const fromSel  = document.getElementById("from");
  const toSel    = document.getElementById("to");
  const amountEl = document.getElementById("amount");
  const resultEl = document.getElementById("result");
  const metaEl   = document.getElementById("result-meta");
  const rateEl   = document.getElementById("rateInfo");
  const dotEl    = document.getElementById("rateDot");
  const btnEl    = document.getElementById("convertBtn");
  const errEl    = document.getElementById("errorStrip");
  const swapEl   = document.getElementById("swapBtn");

  /* ── Populate dropdowns ── */
  CURRENCIES.forEach(([code, name]) => {
    [fromSel, toSel].forEach(sel => {
      const opt = document.createElement("option");
      opt.value = code;
      opt.textContent = `${code} — ${name}`;
      sel.appendChild(opt);
    });
  });

  fromSel.value = "GBP";
  toSel.value   = "USD";

  /* ── Swap currencies ── */
  swapEl.addEventListener("click", function () {
    const tmp = fromSel.value;
    fromSel.value = toSel.value;
    toSel.value = tmp;
    resetResult();
  });

  /* ── Convert on button click ── */
  btnEl.addEventListener("click", convert);

  /* ── Convert on Enter key ── */
  amountEl.addEventListener("keydown", function (e) {
    if (e.key === "Enter") convert();
  });

  /* ── Main convert function ── */
  async function convert() {
    const amount = parseFloat(amountEl.value);
    const from   = fromSel.value;
    const to     = toSel.value;

    errEl.classList.remove("visible");

    if (!amount || isNaN(amount) || amount <= 0) {
      showError("Please enter a valid amount greater than zero.");
      return;
    }

    setLoading(true);

    try {
      const response = await fetch(
        `https://v6.exchangerate-api.com/v6/${API_KEY}/pair/${from}/${to}`
      );
      const data = await response.json();

      console.log(data);
      console.log("Conversion rate:", data.conversion_rate);

      if (data.result !== "success") {
        throw new Error(data["error-type"] || "API error");
      }

      const rate      = data.conversion_rate;
      const converted = (amount * rate).toFixed(2);
      const formatted = parseFloat(converted).toLocaleString("en-GB", {
        minimumFractionDigits: 2,
        maximumFractionDigits: 2
      });

      resultEl.textContent = `${formatted} ${to}`;
      resultEl.classList.remove("empty");

      metaEl.textContent = `${amount.toLocaleString()} ${from} · ${new Date().toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit"
      })}`;

      rateEl.textContent   = `1 ${from} = ${rate.toFixed(6)} ${to}`;
      dotEl.style.opacity  = "1";

    } catch (e) {
      showError("Could not fetch rate. Check your connection or API key.");
      rateEl.textContent  = "Rate unavailable";
      dotEl.style.opacity = "0.15";
    } finally {
      setLoading(false);
    }
  }

  /* ── Helpers ── */
  function setLoading(state) {
    const label = btnEl.querySelector("span");
    if (state) {
      btnEl.classList.add("loading");
      label.textContent    = "Fetching…";
      resultEl.classList.add("empty");
      resultEl.textContent = "—";
      metaEl.textContent   = "";
      rateEl.textContent   = "Fetching live rate…";
      dotEl.style.opacity  = "0.2";
    } else {
      btnEl.classList.remove("loading");
      label.textContent = "Convert";
    }
  }

  function showError(msg) {
    errEl.textContent = msg;
    errEl.classList.add("visible");
  }

  function resetResult() {
    resultEl.textContent = "—";
    resultEl.classList.add("empty");
    metaEl.textContent   = "";
    rateEl.textContent   = "Enter an amount and convert";
    dotEl.style.opacity  = "0.2";
    errEl.classList.remove("visible");
  }

});
