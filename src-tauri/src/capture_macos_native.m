// User-initiated region selection. No screen pixels are read until a selection
// finishes. ScreenCaptureKit samples just that rectangle into process memory.
#import <AppKit/AppKit.h>
#import <CoreImage/CoreImage.h>
#import <CoreMedia/CoreMedia.h>
#import <ImageIO/ImageIO.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <stdbool.h>

enum { MoyuCancelled, MoyuCaptured, MoyuPermissionDenied, MoyuTooLarge, MoyuCaptureError };
static const size_t MaxPNGBytes = 32 * 1024 * 1024;

// NSView points are local to the selected monitor, with y increasing upward.
// ScreenCaptureKit uses local points from the top left. Round inward to avoid
// including a pixel outside the user's rectangle on a Retina display.
bool moyu_display_region(double width, double height, double scale,
                         double x1, double y1, double x2, double y2, double rect[4]) {
    if (!isfinite(scale) || scale <= 0 || width <= 0 || height <= 0) return false;
    double left = ceil(MAX(0, MIN(x1, x2)) * scale) / scale;
    double right = floor(MIN(width, MAX(x1, x2)) * scale) / scale;
    double bottom = ceil(MAX(0, MIN(y1, y2)) * scale) / scale;
    double top = floor(MIN(height, MAX(y1, y2)) * scale) / scale;
    if (right - left < 2 || top - bottom < 2) return false;
    rect[0] = left;
    rect[1] = height - top;
    rect[2] = right - left;
    rect[3] = top - bottom;
    return true;
}

@interface MoyuSelectionPanel : NSPanel
@end
@implementation MoyuSelectionPanel
- (BOOL)canBecomeKeyWindow { return YES; }
@end

@interface MoyuSelectionView : NSView
@property(nonatomic, copy) void (^selected)(CGRect);
@property(nonatomic) NSPoint anchor;
@property(nonatomic) CGRect region;
@property(nonatomic) BOOL dragging;
@end

@implementation MoyuSelectionView
- (BOOL)acceptsFirstResponder { return YES; }
- (BOOL)acceptsFirstMouse:(NSEvent *)event { return YES; }
- (void)resetCursorRects { [self addCursorRect:self.bounds cursor:NSCursor.crosshairCursor]; }
- (void)mouseDown:(NSEvent *)event {
    self.anchor = [self convertPoint:event.locationInWindow fromView:nil];
    self.region = CGRectZero;
    self.dragging = YES;
    self.needsDisplay = YES;
}
- (void)mouseDragged:(NSEvent *)event {
    if (!self.dragging) return;
    NSPoint end = [self convertPoint:event.locationInWindow fromView:nil];
    double rect[4];
    BOOL valid = moyu_display_region(NSWidth(self.bounds), NSHeight(self.bounds),
                                     self.window.backingScaleFactor, self.anchor.x, self.anchor.y,
                                     end.x, end.y, rect);
    self.region = valid ? CGRectMake(rect[0], rect[1], rect[2], rect[3]) : CGRectZero;
    self.needsDisplay = YES;
}
- (void)mouseUp:(NSEvent *)event {
    if (!self.dragging) return;
    [self mouseDragged:event];
    self.dragging = NO;
    self.selected(self.region);
}
- (void)rightMouseDown:(NSEvent *)event { self.selected(CGRectZero); }
- (void)cancelOperation:(id)sender { self.selected(CGRectZero); }
- (void)keyDown:(NSEvent *)event {
    if (event.keyCode == 53) [self cancelOperation:self];
    else [super keyDown:event];
}
- (void)drawRect:(NSRect)dirtyRect {
    [[NSColor colorWithWhite:0 alpha:0.25] setFill];
    NSRectFillUsingOperation(self.bounds, NSCompositingOperationCopy);
    if (!CGRectIsEmpty(self.region)) {
        NSRect box = NSMakeRect(self.region.origin.x, NSHeight(self.bounds) - CGRectGetMaxY(self.region),
                               self.region.size.width, self.region.size.height);
        [[NSColor clearColor] setFill];
        NSRectFillUsingOperation(box, NSCompositingOperationCopy);
        [[NSColor whiteColor] setStroke];
        [NSBezierPath strokeRect:box];
    }
}
@end

// All selection session and NSWindow access happens on AppKit's main thread.
@interface MoyuSelection : NSObject
@property(nonatomic, strong) NSMutableArray<NSPanel *> *panels;
@property(nonatomic, strong) dispatch_semaphore_t done;
@property(nonatomic) CGDirectDisplayID displayID;
@property(nonatomic) CGFloat scale;
@property(nonatomic) CGRect region;
@property(nonatomic) BOOL finished;
@property(nonatomic) BOOL screenChanged;
@property(nonatomic, strong) NSRunningApplication *previousApp;
@property(nonatomic, strong) NSWindow *mainWindow;
@property(nonatomic) BOOL wasHidden, wasVisible, wasFocused;
@end

@implementation MoyuSelection
- (void)finish {
    if (self.finished) return;
    self.finished = YES;
    for (NSPanel *panel in self.panels) [panel orderOut:nil];
    dispatch_semaphore_signal(self.done);
}
- (void)displayChanged:(NSNotification *)note { self.screenChanged = YES; [self finish]; }
- (void)lostFocus:(NSNotification *)note { [self finish]; }

- (void)presentForWindow:(NSWindow *)window {
    self.mainWindow = window;
    self.previousApp = NSWorkspace.sharedWorkspace.frontmostApplication;
    self.wasHidden = NSApp.hidden;
    if (self.wasHidden) [NSApp unhideWithoutActivation];
    self.wasVisible = window.visible && !window.miniaturized;
    self.wasFocused = window.keyWindow;
    if (self.wasVisible) [window orderOut:nil];
    self.panels = [NSMutableArray new];
    NSNotificationCenter *notifications = NSNotificationCenter.defaultCenter;
    [notifications addObserver:self selector:@selector(displayChanged:) name:NSApplicationDidChangeScreenParametersNotification object:nil];
    [notifications addObserver:self selector:@selector(lostFocus:) name:NSApplicationDidResignActiveNotification object:NSApp];
    NSPanel *activePanel = nil;
    for (NSScreen *screen in NSScreen.screens) {
        NSRect frame = screen.frame;
        CGDirectDisplayID display = [screen.deviceDescription[@"NSScreenNumber"] unsignedIntValue];
        if (!display) continue;
        NSPanel *panel = [[MoyuSelectionPanel alloc] initWithContentRect:frame styleMask:NSWindowStyleMaskBorderless
            backing:NSBackingStoreBuffered defer:NO];
        panel.title = @"墨译截屏选区";
        panel.releasedWhenClosed = NO;
        panel.opaque = NO;
        panel.backgroundColor = NSColor.clearColor;
        panel.hasShadow = NO;
        panel.hidesOnDeactivate = NO;
        panel.level = NSScreenSaverWindowLevel;
        panel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                                   NSWindowCollectionBehaviorFullScreenAuxiliary |
                                   NSWindowCollectionBehaviorStationary |
                                   NSWindowCollectionBehaviorIgnoresCycle;
        [panel setFrame:frame display:NO];
        MoyuSelectionView *view = [[MoyuSelectionView alloc] initWithFrame:NSMakeRect(0, 0, NSWidth(frame), NSHeight(frame))];
        CGFloat scale = screen.backingScaleFactor;
        __weak MoyuSelection *weakSelf = self;
        view.selected = ^(CGRect region) {
            MoyuSelection *selection = weakSelf;
            if (!selection || selection.finished) return;
            selection.displayID = display;
            selection.scale = scale;
            selection.region = region;
            [selection finish];
        };
        panel.contentView = view;
        [panel makeFirstResponder:view];
        [self.panels addObject:panel];
        if (NSPointInRect(NSEvent.mouseLocation, frame)) activePanel = panel;
        [panel orderFrontRegardless];
    }
    if (!self.panels.count) { [self finish]; return; }
    [NSApp activateIgnoringOtherApps:YES];
    [(activePanel ?: self.panels.firstObject) makeKeyAndOrderFront:nil];
}

- (void)restoreAfter:(int)result {
    [self finish];
    [NSNotificationCenter.defaultCenter removeObserver:self];
    for (NSPanel *panel in self.panels) [panel close];
    [self.panels removeAllObjects];
    NSWindow *window = self.mainWindow;
    if (result != MoyuCancelled) {
        [NSApp unhideWithoutActivation];
        if (window.miniaturized) [window deminiaturize:nil];
        [NSApp activateIgnoringOtherApps:YES];
        [window makeKeyAndOrderFront:nil];
        return;
    }
    if (self.wasVisible) {
        if (self.wasFocused && NSApp.active) [window makeKeyAndOrderFront:nil];
        else [window orderFront:nil];
    }
    if (self.wasHidden) [NSApp hide:nil];
    // If the user switched apps to cancel, respect that new foreground app.
    if (NSApp.active && self.previousApp && ![self.previousApp isEqual:NSRunningApplication.currentApplication])
        [self.previousApp activateWithOptions:0];
}
@end

static bool screen_capture_allowed(void) {
    return CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess();
}

@interface MoyuFrame : NSObject <SCStreamOutput, SCStreamDelegate>
@property(nonatomic, strong) dispatch_semaphore_t ready;
@property(nonatomic, strong) CIImage *image;
@property(nonatomic) BOOL finished;
@end

@implementation MoyuFrame
- (void)finishWithImage:(CIImage *)image {
    @synchronized (self) {
        if (self.finished) return;
        self.finished = YES;
        self.image = image;
        dispatch_semaphore_signal(self.ready);
    }
}
- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
    [self finishWithImage:nil];
}
- (void)stream:(SCStream *)stream didOutputSampleBuffer:(CMSampleBufferRef)sample ofType:(SCStreamOutputType)type {
    if (type != SCStreamOutputTypeScreen || !CMSampleBufferIsValid(sample)) return;
    NSArray *attachments = (__bridge NSArray *)CMSampleBufferGetSampleAttachmentsArray(sample, false);
    NSNumber *status = attachments.firstObject[SCStreamFrameInfoStatus];
    CVImageBufferRef buffer = CMSampleBufferGetImageBuffer(sample);
    if (status && status.integerValue == SCFrameStatusComplete && buffer)
        [self finishWithImage:[CIImage imageWithCVPixelBuffer:buffer]];
}
@end

static int encode_selected_image(CIImage *image, NSInteger width, NSInteger height, NSData **png) {
    if (!image || CGRectGetWidth(image.extent) < width || CGRectGetHeight(image.extent) < height) return MoyuCaptureError;
    CIContext *context = [CIContext contextWithOptions:nil];
    CGImageRef rgb = [context createCGImage:image fromRect:CGRectMake(0, 0, width, height)];
    if (!rgb) return MoyuCaptureError;
    NSMutableData *data = [NSMutableData data];
    CGImageDestinationRef encoder = CGImageDestinationCreateWithData((__bridge CFMutableDataRef)data, CFSTR("public.png"), 1, NULL);
    BOOL encoded = NO;
    if (encoder) {
        CGImageDestinationAddImage(encoder, rgb, NULL);
        encoded = CGImageDestinationFinalize(encoder);
        CFRelease(encoder);
    }
    CGImageRelease(rgb);
    if (!encoded || !data.length) return MoyuCaptureError;
    if (data.length > MaxPNGBytes) return MoyuTooLarge;
    *png = data;
    return MoyuCaptured;
}

static int capture_selected_png(CGDirectDisplayID displayID, CGRect region, CGFloat scale, NSData **png) {
    NSInteger width = lround(region.size.width * scale), height = lround(region.size.height * scale);
    if (width < 1 || height < 1 || width > 8192 || height > 8192) return MoyuTooLarge;
    dispatch_semaphore_t found = dispatch_semaphore_create(0);
    __block SCShareableContent *available;
    [SCShareableContent getShareableContentExcludingDesktopWindows:NO onScreenWindowsOnly:YES
        completionHandler:^(SCShareableContent *content, NSError *error) {
            if (!error) available = content;
            dispatch_semaphore_signal(found);
        }];
    if (dispatch_semaphore_wait(found, dispatch_time(DISPATCH_TIME_NOW, 12 * NSEC_PER_SEC)) != 0) return MoyuCaptureError;
    SCDisplay *display = nil;
    for (SCDisplay *candidate in available.displays)
        if (candidate.displayID == displayID) { display = candidate; break; }
    if (!display || !CGRectContainsRect(CGRectMake(0, 0, display.width, display.height), region)) return MoyuCaptureError;
    NSMutableArray *excluded = [NSMutableArray new];
    for (SCRunningApplication *app in available.applications)
        if (app.processID == NSProcessInfo.processInfo.processIdentifier) [excluded addObject:app];
    SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:display excludingApplications:excluded exceptingWindows:@[]];
    SCStreamConfiguration *config = [SCStreamConfiguration new];
    config.sourceRect = region;
    config.width = width;
    config.height = height;
    config.showsCursor = NO;
    config.capturesAudio = NO;
    config.pixelFormat = kCVPixelFormatType_32BGRA;
    config.queueDepth = 3;
    MoyuFrame *frame = [MoyuFrame new];
    frame.ready = dispatch_semaphore_create(0);
    dispatch_queue_t queue = dispatch_queue_create("com.moyu.translate.selected-frame", DISPATCH_QUEUE_SERIAL);
    SCStream *stream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:frame];
    if (![stream addStreamOutput:frame type:SCStreamOutputTypeScreen sampleHandlerQueue:queue error:nil])
        return MoyuCaptureError;
    [stream startCaptureWithCompletionHandler:^(NSError *error) {
        if (error) [frame finishWithImage:nil];
        else {
            // A slow start can finish after the worker already timed out.
            @synchronized (frame) {
                if (frame.finished && !frame.image) [stream stopCaptureWithCompletionHandler:nil];
            }
        }
    }];
    dispatch_semaphore_wait(frame.ready, dispatch_time(DISPATCH_TIME_NOW, 12 * NSEC_PER_SEC));
    [frame finishWithImage:nil];
    [stream stopCaptureWithCompletionHandler:nil];
    [stream removeStreamOutput:frame type:SCStreamOutputTypeScreen error:nil];
    return encode_selected_image(frame.image, width, height, png);
}

// Called synchronously from a Rust worker thread. The callback borrows the PNG
// only for the duration of this call. Cancelling never creates a capture stream.
int moyu_capture_region(void *mainWindow, void *context, void (*receive)(void *, const uint8_t *, size_t)) {
    @autoreleasepool {
        if (NSThread.isMainThread || !mainWindow) return MoyuCaptureError;
        if (!screen_capture_allowed()) return MoyuPermissionDenied;
        MoyuSelection *selection = [MoyuSelection new];
        selection.done = dispatch_semaphore_create(0);
        dispatch_sync(dispatch_get_main_queue(), ^{ [selection presentForWindow:(__bridge NSWindow *)mainWindow]; });
        dispatch_semaphore_wait(selection.done, dispatch_time(DISPATCH_TIME_NOW, 120 * NSEC_PER_SEC));
        dispatch_sync(dispatch_get_main_queue(), ^{ [selection finish]; });
        NSData *png = nil;
        int result = MoyuCancelled;
        if (!CGRectIsEmpty(selection.region)) {
            result = capture_selected_png(selection.displayID, selection.region, selection.scale, &png);
            if (result == MoyuCaptureError && !CGPreflightScreenCaptureAccess()) result = MoyuPermissionDenied;
        }
        __block int finalResult = result;
        dispatch_sync(dispatch_get_main_queue(), ^{
            if (selection.screenChanged) finalResult = MoyuCancelled;
            [selection restoreAfter:finalResult];
        });
        if (finalResult == MoyuCaptured) receive(context, png.bytes, png.length);
        return finalResult;
    }
}
