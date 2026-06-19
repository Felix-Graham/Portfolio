import time
import numpy as np 
import matplotlib.pyplot as plt 
import requests 
import json
import datetime 
from datetime import timedelta


_stock_data_cache = None
_selected_symbol = None


def get_stock_prices(symbol):
    """Fetch stock data from Alpha Vantage API."""
    url = f'https://www.alphavantage.co/query?function=TIME_SERIES_DAILY&symbol={symbol}&apikey=OP54JSLHKZWG2U6J'
    response = requests.get(url)
    data = response.json()
    return data


def get_stock_data(symbol):
    """Get cached stock data with symbol validation."""
    global _stock_data_cache, _selected_symbol
    _selected_symbol = symbol
    
    if _stock_data_cache is None:
        _stock_data_cache = get_stock_prices(symbol)
        
        # Validate the response contains the requested symbol
        try:
            response_symbol = _stock_data_cache["Meta Data"]["2. Symbol"]
            if response_symbol != symbol:
                raise ValueError(
                    f"Symbol mismatch: requested {symbol}, got {response_symbol}"
                )
        except KeyError:
            raise ValueError("Invalid API response: missing metadata or symbol")
    
    return _stock_data_cache


def getPoints(initPrice):
    """Generate future stock price predictions using geometric random walk."""
    mu = 0.001      # drift (slight upward bias)
    sigma = 0.02    # volatility
    price = initPrice
    points = []
    for i in range(100):
        shock = np.random.normal(loc=mu, scale=sigma)
        price = price * (1 + shock)  # percentage change, not absolute
        points.append([i, price])
        if price <= 0:
            break
    return points


def get_closing_by_date_from_now(n):
    """Get closing price from n days ago, handling weekends/holidays."""
    date = datetime.datetime.now() + timedelta(days=-n)
    data = date.strftime("%Y-%m-%d")
    
    d = get_stock_data(_selected_symbol)
    try:
        result = d["Time Series (Daily)"][data]["4. close"]
    except KeyError:
        # Date not found (weekend/holiday), try previous day
        return get_closing_by_date_from_now(n + 1)
    return result


def correctly_format_previous_data(timeMachine):
    """Format historical data with negative x values (past), x=0 is today."""
    thePast = []
    for i in range(timeMachine):
        price = float(get_closing_by_date_from_now(i))
        thePast.append([-i, price])  # Negative index for days in the past
    return thePast


def merge_data(thePast, theFuture):
    """Merge historical and predicted data."""
    return thePast + theFuture


def createGraph():
    """Create 50 Monte Carlo simulations of stock prices with average prediction line."""
    all_simulations = []
    historical_data = None
    
    for idx in range(50):
        predictedPoints = getPoints(int(round(float(get_closing_by_date_from_now(0)))))
        pastPoint = correctly_format_previous_data(len(predictedPoints))
        
        # Store historical data only on first iteration
        if idx == 0:
            historical_data = pastPoint
        
        # Offset predicted points to start from day 1 (tomorrow)
        offset = 1
        predictedPoints = [[p[0] + offset, p[1]] for p in predictedPoints]
        
        points = merge_data(pastPoint, predictedPoints)
        all_simulations.append(points)
        
        # Only plot the predicted (future) part
        xs = [p[0] for p in predictedPoints]
        ys = [p[1] for p in predictedPoints]
        plt.plot(xs, ys, alpha=0.2, color='steelblue')
    
    # Plot historical data once as coherent red line
    if historical_data:
        hist_xs = [p[0] for p in historical_data]
        hist_ys = [p[1] for p in historical_data]
        plt.plot(hist_xs, hist_ys, alpha=0.9, color='red', linewidth=2.5, label='Historical Data')
    
    # Calculate and plot average prediction line (future only)
    if all_simulations:
        max_future_length = max(len([p for p in sim if p[0] > 0]) for sim in all_simulations)
        avg_ys = []
        avg_xs = []
        
        for x_val in range(1, max_future_length + 1):
            prices = [p[1] for sim in all_simulations for p in sim if p[0] == x_val]
            if prices:
                avg_ys.append(np.mean(prices))
                avg_xs.append(x_val)
        
        plt.plot(avg_xs, avg_ys, alpha=0.95, color='red', linewidth=3.5, label='Average Prediction')
    
    # Add vertical line at x=0 to mark present
    plt.axvline(x=0, color='black', linestyle='-', linewidth=2.5, alpha=0.8, label='Present (Today)')
    
    plt.title(f"{_selected_symbol} Stock Price Prediction - Monte Carlo Analysis")
    plt.xlabel("Days from Today (negative = past, positive = future)")
    plt.ylabel("Stock Price ($)")
    plt.axhline(y=0, color='black', linewidth=0.5)
    plt.legend(loc='best')
    plt.grid(alpha=0.3)
    plt.tight_layout()
    plt.show()


def main():
    global _stock_data_cache, _selected_symbol
    
    symbol = input("Enter stock symbol (e.g., IBM, AAPL, TSLA): ").strip().upper()
    if not symbol:
        symbol = "IBM"
    
    # Reset cache for new symbol
    _stock_data_cache = None
    _selected_symbol = symbol
    
    print(f"\nFetching data for {symbol}...")
    try:
        get_stock_data(symbol)
        print(f"Successfully loaded {symbol}. Generating predictions...\n")
        createGraph()
    except ValueError as e:
        print(f"Error: {e}")


if __name__ == "__main__":
    main()
