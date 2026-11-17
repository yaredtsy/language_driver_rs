function forwardRef<P>(render: (props: P) => string): (props: P) => string {
  return render;
}

function label(): string {
  return "button";
}

export const Button = forwardRef((props: { text: string }) => {
  return label();
});
