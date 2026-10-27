// WebDriver does not implement trusted default button activation on WKWebView.
// This models that DOM default action without sending a key to the user's desktop.
export async function activateFocusedButton() {
  return browser.execute(() => {
    const target=document.activeElement;
    if(!(target instanceof HTMLButtonElement))throw new Error('Expected a focused button');
    const allowed=target.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',code:'Enter',bubbles:true,cancelable:true}));
    if(allowed&&!target.disabled)target.click();
    target.dispatchEvent(new KeyboardEvent('keyup',{key:'Enter',code:'Enter',bubbles:true}));
    return {label:target.getAttribute('aria-label')||target.textContent.trim(),trusted:false};
  });
}
