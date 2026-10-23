import { useState } from 'react'
import type { WorkspaceImage } from '../../lib/api'
import './image-viewer.css'

/** SVG stays in an image context: never inject its markup into the document. */
export default function ImageViewer({ image }: { image: WorkspaceImage }) {
  const [zoom, setZoom] = useState<number | null>(null)
  const [decoded, setDecoded] = useState<{ source: string; width: number; height: number } | null>(null)
  const [failedSource, setFailedSource] = useState<string | null>(null)
  const dimensions = decoded?.source === image.dataUrl ? decoded : null
  const failed = failedSource === image.dataUrl
  const scale = (factor: number) => setZoom(value => Math.min(8, Math.max(0.1, (value ?? 1) * factor)))
  return <div className="df-image-viewer">
    <div className="image-viewer-toolbar" role="toolbar" aria-label="图片查看">
      <button type="button" aria-pressed={zoom === null} onClick={() => setZoom(null)}>适应窗口</button>
      <button type="button" aria-pressed={zoom === 1} onClick={() => setZoom(1)}>原始尺寸</button>
      <button type="button" aria-label="缩小图片" disabled={zoom !== null && zoom <= 0.1} onClick={() => scale(1 / 1.25)}>−</button>
      <span>{zoom === null ? '自动' : `${Math.round(zoom * 100)}%`}</span>
      <button type="button" aria-label="放大图片" disabled={zoom !== null && zoom >= 8} onClick={() => scale(1.25)}>+</button>
    </div>
    <div className={`image-viewer-scroll ${zoom === null ? 'is-fit' : ''}`}>
      {failed && <p className="image-viewer-error" role="alert">无法显示图片，文件可能已损坏或当前浏览器不支持此格式。</p>}
      <img src={image.dataUrl} alt={image.path.split(/[\\/]/).at(-1) ?? '图片预览'} hidden={failed}
        draggable={false} style={zoom !== null && dimensions ? { width: dimensions.width * zoom, height: dimensions.height * zoom } : undefined}
        onLoad={event => { setFailedSource(null); setDecoded({ source: image.dataUrl, width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight }) }}
        onError={() => setFailedSource(image.dataUrl)} />
    </div>
    <footer className="file-status"><span>图片预览 · 只读</span>{dimensions && <span>{dimensions.width} × {dimensions.height}</span>}<span>{image.mimeType.replace('image/', '').toUpperCase()} · {image.size < 1024 * 1024 ? `${(image.size / 1024).toFixed(1)} KiB` : `${(image.size / (1024 * 1024)).toFixed(1)} MiB`}</span></footer>
  </div>
}
