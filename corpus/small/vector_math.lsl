vector gHome = <10.0, 20.0, 30.0>;
rotation gTurn = <0.0, 0.0, 0.0, 1.0>;

default
{
    state_entry()
    {
        vector offset = <1.0, 2.0, 3.0>;
        vector scaled = offset * 2.0;
        vector rotated = scaled * gTurn;
        float dot = scaled * offset;
        vector cross = scaled % offset;
        gHome = gHome + rotated - cross;
        gTurn = gTurn * <0.0, 0.0, 0.0, 1.0>;
        llSetPos(gHome);
        llSetRot(gTurn);
        llOwnerSay((string)dot + " " + (string)gHome.x);
    }
}
