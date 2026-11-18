import { useEffect, useRef, useState } from "react";
import { uploadImage } from "./api/client";
import { IMAGE_MAX_BYTES } from "./api/contract";

/// An image added to a follow-up, uploaded the moment it is added.
export interface Attached {
  key: number;
  name: string;
  /// The picture as the browser shows it, from the file itself.
  url: string;
  /// The share of it sent so far, 0 to 1.
  sent: number;
  /// What the send names it by, once the server kept it.
  id: string | null;
  error: string | null;
}

export const TOO_LARGE = `The image is too large: it can be ${IMAGE_MAX_BYTES / 1024 / 1024} MB at most.`;

/// The images among a paste's files: a copied screenshot or photo, never a
/// copied document.
export function pastedImages(files: Iterable<File>): File[] {
  return [...files].filter((f) => f.type.startsWith("image/"));
}

/// What keeps a follow-up with `images` from going, if anything.
export function imagesHold(images: Attached[]): string | null {
  if (images.some((i) => i.error)) return "Remove the image that did not upload first.";
  if (images.some((i) => !i.id)) return "Uploading the image…";
  return null;
}

/// The follow-up's images: each uploaded as it is added, its upload stopped
/// when it is removed or the box goes.
export function useAttached(): {
  images: Attached[];
  add: (files: File[]) => void;
  remove: (key: number) => void;
  clear: () => void;
} {
  const [images, setImages] = useState<Attached[]>([]);
  const seq = useRef(0);
  const uploads = useRef(new Map<number, () => void>());
  const shown = useRef(images);
  shown.current = images;
  useEffect(
    () => () => {
      for (const abort of uploads.current.values()) abort();
      for (const i of shown.current) URL.revokeObjectURL(i.url);
    },
    [],
  );
  const patch = (key: number, p: Partial<Attached>) => setImages((list) => list.map((i) => (i.key === key ? { ...i, ...p } : i)));
  const add = (files: File[]) => {
    const added = files.map((file): Attached => {
      const key = ++seq.current;
      const image = { key, name: file.name, url: URL.createObjectURL(file), sent: 0, id: null, error: null };
      if (file.size > IMAGE_MAX_BYTES) return { ...image, error: TOO_LARGE };
      const up = uploadImage(file, (sent) => patch(key, { sent }));
      uploads.current.set(key, up.abort);
      void up.done.then((r) => {
        uploads.current.delete(key);
        patch(key, r.ok ? { id: r.value.id, sent: 1 } : { error: r.error });
      });
      return image;
    });
    setImages((list) => [...list, ...added]);
  };
  const drop = (gone: Attached[]) => {
    for (const i of gone) {
      uploads.current.get(i.key)?.();
      uploads.current.delete(i.key);
      URL.revokeObjectURL(i.url);
    }
  };
  const remove = (key: number) => {
    drop(images.filter((i) => i.key === key));
    setImages((list) => list.filter((i) => i.key !== key));
  };
  const clear = () => {
    drop(images);
    setImages([]);
  };
  return { images, add, remove, clear };
}

/// The thumbnails of a follow-up's images, each with its upload's progress
/// or why it failed, and a button that removes it.
export function ImageStrip({ images, onRemove }: { images: Attached[]; onRemove: (key: number) => void }) {
  if (images.length === 0) return null;
  const failed = images.find((i) => i.error);
  return (
    <div className="attached">
      <ul className="attached-list" aria-label="Images">
        {images.map((i) => (
          <li key={i.key} className={i.error ? "thumb failed" : i.id ? "thumb" : "thumb uploading"}>
            <Thumb image={i} />
            {!i.id && !i.error && (
              <span className="thumb-bar" role="progressbar" aria-label={`Uploading ${i.name}`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(i.sent * 100)}>
                <span style={{ width: `${Math.max(i.sent, 0.04) * 100}%` }} />
              </span>
            )}
            <button type="button" className="thumb-remove" aria-label={`Remove ${i.name}`} onClick={() => onRemove(i.key)}>
              <svg aria-hidden="true" width="12" height="12" viewBox="0 0 24 24" fill="none" strokeWidth="3" strokeLinecap="round">
                <line x1="6" y1="6" x2="18" y2="18" />
                <line x1="18" y1="6" x2="6" y2="18" />
              </svg>
            </button>
          </li>
        ))}
      </ul>
      {failed && (
        <p className="error" role="alert">
          {failed.error}
        </p>
      )}
    </div>
  );
}

/// The picture, or its name where the browser cannot draw it: a HEIC in
/// Chrome, which the server makes a JPEG all the same.
function Thumb({ image }: { image: Attached }) {
  const [drawn, setDrawn] = useState(true);
  if (!drawn) return <span className="thumb-name">{image.name.split(".").pop()?.toUpperCase() || "Image"}</span>;
  return <img src={image.url} alt={image.name} onError={() => setDrawn(false)} />;
}
