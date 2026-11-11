function View({ title }) {
  return <section>{title}</section>;
}

const Item = ({ label }) => <li>{label}</li>;

class Panel extends Component {
  render() {
    return <div />;
  }
}
