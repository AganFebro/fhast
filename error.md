PS D:\Codes\fhast\extension> npm run build

> fhast-extension@0.1.0 build
> tsc

background.ts:61:1 - error TS2304: Cannot find name 'chrome'.

61 chrome.runtime.onInstalled.addListener(() => {
   ~~~~~~

background.ts:62:3 - error TS2304: Cannot find name 'chrome'.

62   chrome.contextMenus.create({
     ~~~~~~

background.ts:70:1 - error TS2304: Cannot find name 'chrome'.

70 chrome.runtime.onStartup.addListener(() => {
   ~~~~~~

background.ts:75:24 - error TS2304: Cannot find name 'chrome'.

75   const result = await chrome.storage.local.get(STORAGE_KEYS.AUTO_GRAB_ENABLED);
                          ~~~~~~

background.ts:82:1 - error TS2304: Cannot find name 'chrome'.

82 chrome.contextMenus.onClicked.addListener((info, tab) => {
   ~~~~~~

background.ts:82:44 - error TS7006: Parameter 'info' implicitly has an 'any' type.

82 chrome.contextMenus.onClicked.addListener((info, tab) => {
                                              ~~~~

background.ts:82:50 - error TS7006: Parameter 'tab' implicitly has an 'any' type.

82 chrome.contextMenus.onClicked.addListener((info, tab) => {
                                                    ~~~

background.ts:94:1 - error TS2304: Cannot find name 'chrome'.

94 chrome.runtime.onMessage.addListener((request, _sender, sendResponse) => {
   ~~~~~~

background.ts:94:39 - error TS7006: Parameter 'request' implicitly has an 'any' type.

94 chrome.runtime.onMessage.addListener((request, _sender, sendResponse) => {
                                         ~~~~~~~

background.ts:94:48 - error TS7006: Parameter '_sender' implicitly has an 'any' type.

94 chrome.runtime.onMessage.addListener((request, _sender, sendResponse) => {
                                                  ~~~~~~~

background.ts:94:57 - error TS7006: Parameter 'sendResponse' implicitly has an 'any' type.

94 chrome.runtime.onMessage.addListener((request, _sender, sendResponse) => {
                                                           ~~~~~~~~~~~~

background.ts:112:5 - error TS2304: Cannot find name 'chrome'.

112     chrome.storage.local.get(STORAGE_KEYS.RECENT_CAPTURES).then((result) => {
        ~~~~~~

background.ts:112:66 - error TS7006: Parameter 'result' implicitly has an 'any' type.

112     chrome.storage.local.get(STORAGE_KEYS.RECENT_CAPTURES).then((result) => {
                                                                     ~~~~~~

background.ts:120:5 - error TS2304: Cannot find name 'chrome'.

120     chrome.storage.local
        ~~~~~~

background.ts:128:10 - error TS2304: Cannot find name 'chrome'.

128   return chrome.downloads !== undefined;
             ~~~~~~

background.ts:132:10 - error TS2304: Cannot find name 'chrome'.

132   return chrome.webRequest !== undefined;
             ~~~~~~

background.ts:139:25 - error TS2304: Cannot find name 'chrome'.

139   const granted = await chrome.permissions.request({
                            ~~~~~~

background.ts:149:9 - error TS2304: Cannot find name 'chrome'.

149   await chrome.storage.local.set({
            ~~~~~~

background.ts:159:9 - error TS2304: Cannot find name 'chrome'.

159   await chrome.storage.local.set({
            ~~~~~~

background.ts:167:5 - error TS2304: Cannot find name 'chrome'.

167     chrome.webRequest.onBeforeSendHeaders.addListener(
        ~~~~~~

background.ts:172:5 - error TS2304: Cannot find name 'chrome'.

172     chrome.webRequest.onBeforeRedirect.addListener(
        ~~~~~~

background.ts:177:5 - error TS2304: Cannot find name 'chrome'.

177     chrome.webRequest.onHeadersReceived.addListener(
        ~~~~~~

background.ts:186:5 - error TS2304: Cannot find name 'chrome'.

186     chrome.downloads.onDeterminingFilename.addListener(onDeterminingFilename);
        ~~~~~~

background.ts:187:5 - error TS2304: Cannot find name 'chrome'.

187     chrome.downloads.onCreated.addListener(onDownloadCreated);
        ~~~~~~

background.ts:194:5 - error TS2304: Cannot find name 'chrome'.

194     chrome.webRequest.onBeforeSendHeaders.removeListener(onBeforeSendHeaders);
        ~~~~~~

background.ts:195:5 - error TS2304: Cannot find name 'chrome'.

195     chrome.webRequest.onBeforeRedirect.removeListener(onBeforeRedirect);
        ~~~~~~

background.ts:196:5 - error TS2304: Cannot find name 'chrome'.

196     chrome.webRequest.onHeadersReceived.removeListener(onHeadersReceived);
        ~~~~~~

background.ts:201:5 - error TS2304: Cannot find name 'chrome'.

201     chrome.downloads.onDeterminingFilename.removeListener(
        ~~~~~~

background.ts:204:5 - error TS2304: Cannot find name 'chrome'.

204     chrome.downloads.onCreated.removeListener(onDownloadCreated);
        ~~~~~~

background.ts:213:12 - error TS2503: Cannot find namespace 'chrome'.

213   details: chrome.webRequest.WebRequestHeadersDetails,
               ~~~~~~

background.ts:238:12 - error TS2503: Cannot find namespace 'chrome'.

238   details: chrome.webRequest.WebResponseHeadersDetails,
               ~~~~~~

background.ts:299:12 - error TS2503: Cannot find namespace 'chrome'.

299   details: chrome.webRequest.WebRedirectionResponseDetails,
               ~~~~~~

background.ts:332:17 - error TS2503: Cannot find namespace 'chrome'.

332   downloadItem: chrome.downloads.DownloadItem,
                    ~~~~~~

background.ts:336:3 - error TS2304: Cannot find name 'chrome'.

336   chrome.downloads.cancel(downloadItem.id, () => {
      ~~~~~~

background.ts:337:9 - error TS2304: Cannot find name 'chrome'.

337     if (chrome.runtime.lastError) {
            ~~~~~~

background.ts:340:9 - error TS2304: Cannot find name 'chrome'.

340         chrome.runtime.lastError.message,
            ~~~~~~

background.ts:349:17 - error TS2503: Cannot find namespace 'chrome'.

349   downloadItem: chrome.downloads.DownloadItem,
                    ~~~~~~

background.ts:350:26 - error TS2503: Cannot find namespace 'chrome'.

350   suggest: (suggestion?: chrome.downloads.DownloadFilenameSuggestion) => void,
                             ~~~~~~

background.ts:360:3 - error TS2304: Cannot find name 'chrome'.

360   chrome.downloads.cancel(downloadItem.id, () => {
      ~~~~~~

background.ts:361:9 - error TS2304: Cannot find name 'chrome'.

361     if (chrome.runtime.lastError) {
            ~~~~~~

background.ts:364:9 - error TS2304: Cannot find name 'chrome'.

364         chrome.runtime.lastError.message,
            ~~~~~~

background.ts:378:17 - error TS2503: Cannot find namespace 'chrome'.

378   downloadItem: chrome.downloads.DownloadItem,
                    ~~~~~~

background.ts:481:24 - error TS2304: Cannot find name 'chrome'.

481   const result = await chrome.storage.local.get(STORAGE_KEYS.RECENT_CAPTURES);
                           ~~~~~~

background.ts:487:9 - error TS2304: Cannot find name 'chrome'.

487   await chrome.storage.local.set({
            ~~~~~~

background.ts:492:32 - error TS2503: Cannot find namespace 'chrome'.

492 function parseHeaders(headers: chrome.webRequest.HttpHeader[]): {
                                   ~~~~~~

background.ts:535:3 - error TS2304: Cannot find name 'chrome'.

535   chrome.runtime.sendNativeMessage(
      ~~~~~~

background.ts:539:11 - error TS2304: Cannot find name 'chrome'.

539       if (chrome.runtime.lastError) {
              ~~~~~~

background.ts:542:11 - error TS2304: Cannot find name 'chrome'.

542           chrome.runtime.lastError.message,
              ~~~~~~

candidates.ts:119:12 - error TS2503: Cannot find namespace 'chrome'.

119   headers: chrome.webRequest.HttpHeader[],
               ~~~~~~

candidates.ts:167:21 - error TS2503: Cannot find namespace 'chrome'.

167   responseHeaders?: chrome.webRequest.HttpHeader[],
                        ~~~~~~

popup.ts:35:42 - error TS2304: Cannot find name 'chrome'.

35     const status: AutoGrabStatus = await chrome.runtime.sendMessage({
                                            ~~~~~~

popup.ts:61:42 - error TS2304: Cannot find name 'chrome'.

61     const status: AutoGrabStatus = await chrome.runtime.sendMessage({
                                            ~~~~~~

popup.ts:66:13 - error TS2304: Cannot find name 'chrome'.

66       await chrome.runtime.sendMessage({ action: "disableAutoGrab" });
               ~~~~~~

popup.ts:70:28 - error TS2304: Cannot find name 'chrome'.

70       const result = await chrome.runtime.sendMessage({
                              ~~~~~~

popup.ts:86:26 - error TS2304: Cannot find name 'chrome'.

86     const result = await chrome.runtime.sendMessage({
                            ~~~~~~

popup.ts:154:5 - error TS2304: Cannot find name 'chrome'.

154     chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
        ~~~~~~

popup.ts:154:63 - error TS7006: Parameter 'tabs' implicitly has an 'any' type.

154     chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
                                                                  ~~~~

popup.ts:177:5 - error TS2304: Cannot find name 'chrome'.

177     chrome.runtime.sendNativeMessage(
        ~~~~~~

popup.ts:181:13 - error TS2304: Cannot find name 'chrome'.

181         if (chrome.runtime.lastError) {
                ~~~~~~

popup.ts:183:37 - error TS2304: Cannot find name 'chrome'.

183             "Native host error: " + chrome.runtime.lastError.message,
                                        ~~~~~~


Found 60 errors in 3 files.

Errors  Files
    48  background.ts:61
     2  candidates.ts:119
    10  popup.ts:35
