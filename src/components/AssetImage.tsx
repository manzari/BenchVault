import { Image, Skeleton } from "@mantine/core";
import { useEffect, useState } from "react";
import { api } from "../lib/api";

interface Props {
  assetId: number;
  alt?: string;
  fit?: "contain" | "cover" | "fill" | "scale-down" | "none";
  radius?: string | number;
  height?: number | string;
  width?: number | string;
}

export function AssetImage({ assetId, alt, fit = "cover", radius, height, width }: Props) {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    let revoke: string | null = null;
    let cancelled = false;
    api
      .readAssetBytes(assetId)
      .then((bytes) => {
        if (cancelled) return;
        const blob = new Blob([new Uint8Array(bytes)]);
        const objectUrl = URL.createObjectURL(blob);
        revoke = objectUrl;
        setUrl(objectUrl);
      })
      .catch((e) => {
        // Swallow (e.g. asset deleted on disk) instead of an unhandled
        // rejection; the Skeleton fallback stays visible.
        if (!cancelled) console.warn(`failed to load asset ${assetId}`, e);
      });
    return () => {
      cancelled = true;
      if (revoke) URL.revokeObjectURL(revoke);
    };
  }, [assetId]);

  if (!url) {
    return <Skeleton height={height ?? "100%"} width={width ?? "100%"} radius={radius} />;
  }
  return <Image src={url} alt={alt} fit={fit} radius={radius} h={height} w={width} />;
}
