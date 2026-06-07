const puppeteer = require('puppeteer');

(async () => {
  const browser = await puppeteer.launch();
  const page = await browser.newPage();
  
  // Listen to console and page errors
  page.on('console', msg => console.log('PAGE LOG:', msg.text()));
  page.on('pageerror', error => console.log('PAGE ERROR:', error.message));
  page.on('requestfailed', request => {
    console.log('REQUEST FAILED:', request.url(), request.failure().errorText);
  });
  
  await page.goto('http://127.0.0.1:3036/', {waitUntil: 'networkidle0'});
  console.log('Page loaded.');
  
  // Try to click the first Quick Action or call findConnections directly
  await page.evaluate(() => {
    console.log('Checking window scope for findConnections:', typeof window.findConnections);
    if (typeof window.findConnections === 'function') {
        window.findConnections().catch(e => console.log('PROMISE REJECTION:', e.message));
    }
  });
  
  await new Promise(r => setTimeout(r, 2000));
  await browser.close();
})();
