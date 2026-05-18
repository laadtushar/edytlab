import { useEffect, useRef } from "react";

interface SpectrumPoint {
  hz: number;
  db: number;
}
interface SpectrumChartProps {
  points: SpectrumPoint[];
  width?: number;
  height?: number;
}

export function SpectrumChart({
  points,
  width = 400,
  height = 200,
}: SpectrumChartProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || points.length === 0) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = "#1a1a1a";
    ctx.fillRect(0, 0, width, height);

    const maxHz = points[points.length - 1]?.hz ?? 22050;
    const minDb = -120;
    const maxDb = 0;

    ctx.strokeStyle = "#7c3aed";
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    points.forEach((p, i) => {
      const x = (p.hz / maxHz) * width;
      const y = height - ((p.db - minDb) / (maxDb - minDb)) * height;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.stroke();

    ctx.fillStyle = "#666";
    ctx.font = "10px monospace";
    ctx.fillText("0", 2, height - 2);
    ctx.fillText(`${(maxHz / 1000).toFixed(0)}k`, width - 24, height - 2);
    ctx.fillText("0dB", 2, 10);
    ctx.fillText("-120", 2, height - 12);
  }, [points, width, height]);

  return (
    <canvas ref={canvasRef} width={width} height={height} className="rounded" />
  );
}
