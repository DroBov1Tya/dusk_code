ObjC.import('Cocoa');
// The ring where the agent last clicked: the file named in the argument holds "x y" in
// display points; the helper moves the ring when it changes and ends when it is gone.
function run(argv) {
  const path = argv[0];
  const app = $.NSApplication.sharedApplication;
  app.setActivationPolicy($.NSApplicationActivationPolicyAccessory);
  const size = 34;
  const win = $.NSWindow.alloc.initWithContentRectStyleMaskBackingDefer(
    $.NSMakeRect(0, 0, size, size), $.NSWindowStyleMaskBorderless, $.NSBackingStoreBuffered, false);
  win.setOpaque(false);
  win.setBackgroundColor($.NSColor.clearColor);
  win.setIgnoresMouseEvents(true);
  win.setHasShadow(false);
  win.setLevel($.NSScreenSaverWindowLevel);
  win.setSharingType($.NSWindowSharingNone);
  win.setCollectionBehavior($.NSWindowCollectionBehaviorCanJoinAllSpaces | $.NSWindowCollectionBehaviorStationary);
  const color = (r, g, b, a) => $.NSColor.colorWithSRGBRedGreenBlueAlpha(r, g, b, a);
  const ring = $.NSBox.alloc.initWithFrame($.NSMakeRect(3, 3, size - 6, size - 6));
  ring.setBoxType($.NSBoxCustom);
  ring.setBorderWidth(3);
  ring.setCornerRadius((size - 6) / 2);
  ring.setBorderColor(color(1.0, 0.72, 0.3, 0.95));
  ring.setFillColor(color(0.62, 0.48, 1.0, 0.22));
  win.contentView.addSubview(ring);
  const dot = $.NSBox.alloc.initWithFrame($.NSMakeRect(size / 2 - 3, size / 2 - 3, 6, 6));
  dot.setBoxType($.NSBoxCustom);
  dot.setBorderWidth(0);
  dot.setCornerRadius(3);
  dot.setFillColor(color(0.62, 0.48, 1.0, 1.0));
  win.contentView.addSubview(dot);
  let last = '';
  let seen = Date.now();
  while (true) {
    const text = $.NSString.stringWithContentsOfFileEncodingError(path, $.NSUTF8StringEncoding, null);
    if (text.isNil()) {
      if (Date.now() - seen > 3000) break;
    } else {
      seen = Date.now();
      const line = text.js.trim();
      if (line !== last) {
        last = line;
        const [x, y] = line.split(' ').map(Number);
        const height = $.NSScreen.screens.objectAtIndex(0).frame.size.height;
        win.setFrameOrigin($.NSMakePoint(x - size / 2, height - y - size / 2));
        win.orderFrontRegardless;
      }
    }
    $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(0.1));
  }
}
