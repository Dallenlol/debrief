import { cva, type VariantProps } from "class-variance-authority";
import type { ButtonHTMLAttributes } from "react";
import { cn } from "@/lib/cn";

const button = cva(
  "focus-ring inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md font-medium transition-[background,color,transform,box-shadow] duration-150 active:translate-y-px disabled:pointer-events-none disabled:opacity-50 cursor-pointer",
  {
    variants: {
      variant: {
        primary: "bg-ink text-canvas hover:bg-ink/90 shadow-card",
        ember: "bg-ember text-white hover:bg-ember-2 shadow-card",
        soft: "bg-canvas-3 text-ink hover:bg-line-2",
        ghost: "text-ink-2 hover:bg-canvas-3 hover:text-ink",
        outline: "border border-line-2 bg-canvas text-ink hover:bg-canvas-2",
        danger: "text-ember hover:bg-ember-soft",
      },
      size: {
        sm: "h-8 px-3 text-[13px]",
        md: "h-9 px-4 text-sm",
        lg: "h-11 px-5 text-[15px]",
        icon: "h-9 w-9",
        iconSm: "h-8 w-8",
      },
    },
    defaultVariants: { variant: "soft", size: "md" },
  },
);

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof button> {}

export function Button({ className, variant, size, type = "button", ...props }: ButtonProps) {
  return <button type={type} className={cn(button({ variant, size }), className)} {...props} />;
}
