import {
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  useEffect,
  useRef,
  useState,
} from "react";

type WorkspaceLayoutProps = Readonly<{
  library: ReactNode;
  contents: ReactNode;
  editor: ReactNode;
  contentsVisible: boolean;
  onContentsVisibleChange(visible: boolean): void;
}>;

const MIN_CONTENTS_WIDTH = 220;
const MAX_CONTENTS_WIDTH = 440;

export function WorkspaceLayout({
  library,
  contents,
  editor,
  contentsVisible,
  onContentsVisibleChange,
}: WorkspaceLayoutProps) {
  const [contentsWidth, setContentsWidth] = useState(280);
  const contentsPane = useRef<HTMLElement>(null);
  const showContentsButton = useRef<HTMLButtonElement>(null);
  const wasContentsVisible = useRef(contentsVisible);
  const stopResize = useRef<(() => void) | null>(null);

  useEffect(() => () => stopResize.current?.(), []);

  useEffect(() => {
    if (!contentsVisible) {
      showContentsButton.current?.focus();
    } else if (!wasContentsVisible.current) {
      const selectedDocument = contentsPane.current?.querySelector<HTMLElement>(
        '[role="option"][aria-selected="true"]',
      );
      (selectedDocument ?? contentsPane.current)?.focus();
    }
    wasContentsVisible.current = contentsVisible;
  }, [contentsVisible]);

  function beginResize(event: ReactPointerEvent<HTMLDivElement>) {
    stopResize.current?.();
    const startX = event.clientX;
    const startWidth = contentsWidth;
    const pointerId = event.pointerId;
    const resizer = event.currentTarget;
    resizer.setPointerCapture(pointerId);

    const move = (moveEvent: PointerEvent) => {
      const width = Math.min(
        MAX_CONTENTS_WIDTH,
        Math.max(MIN_CONTENTS_WIDTH, startWidth + moveEvent.clientX - startX),
      );
      setContentsWidth(width);
    };
    const finish = () => {
      globalThis.removeEventListener("pointermove", move);
      globalThis.removeEventListener("pointerup", finish);
      globalThis.removeEventListener("pointercancel", finish);
      if (resizer.hasPointerCapture(pointerId)) {
        resizer.releasePointerCapture(pointerId);
      }
      stopResize.current = null;
    };
    globalThis.addEventListener("pointermove", move);
    globalThis.addEventListener("pointerup", finish);
    globalThis.addEventListener("pointercancel", finish);
    stopResize.current = finish;
  }

  return (
    <div
      className="workspace-layout"
      data-contents-visible={contentsVisible}
      style={{ "--contents-width": `${contentsWidth}px` } as CSSProperties}
    >
      <aside className="library-pane" aria-label="Library">
        {library}
      </aside>
      {contentsVisible && (
        <>
          <aside
            className="contents-pane"
            aria-label="Project contents"
            ref={contentsPane}
            tabIndex={-1}
          >
            {contents}
          </aside>
          <div
            className="pane-resizer"
            role="separator"
            aria-label="Resize project contents"
            aria-orientation="vertical"
            aria-valuemin={MIN_CONTENTS_WIDTH}
            aria-valuemax={MAX_CONTENTS_WIDTH}
            aria-valuenow={contentsWidth}
            tabIndex={0}
            onPointerDown={beginResize}
            onKeyDown={(event) => {
              if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
                event.preventDefault();
                setContentsWidth((width) =>
                  Math.min(
                    MAX_CONTENTS_WIDTH,
                    Math.max(
                      MIN_CONTENTS_WIDTH,
                      width + (event.key === "ArrowLeft" ? -16 : 16),
                    ),
                  ),
                );
              }
            }}
          />
        </>
      )}
      <section className="editor-pane" aria-label="Document editor">
        {!contentsVisible && (
          <button
            className="show-contents-button"
            type="button"
            ref={showContentsButton}
            onClick={() => onContentsVisibleChange(true)}
          >
            Show contents
          </button>
        )}
        {editor}
      </section>
    </div>
  );
}
