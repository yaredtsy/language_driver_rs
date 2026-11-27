function save() {}
function Btn(props: { onPress: () => void; reset: () => void }) {
    props.reset();
    props.onPress();
    return <b />;
}
export function App() {
    return (
        <div>
            <Btn onPress={save} reset={() => {}} />
            <Btn onPress={save} reset={function () {}} />
        </div>
    );
}
